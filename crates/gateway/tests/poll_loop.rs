//! 端到端集成测试：虚拟总线上的完整采集链路。
//!
//! 覆盖：Modbus 驱动编译 → 通道监督器 → 事务串行 → 解码 → TagHub 值/质量 →
//! 写点 → 设备离线/恢复 → 坏设备不阻塞同总线好设备。

use hmi_domain::config::{ChannelConfig, ChannelTiming, DeviceConfig, SimParams, TransportConfig};
use hmi_domain::driver::Driver;
use hmi_domain::value::{Quality, Value};
use hmi_driver_modbus::{ModbusDriver, SimSlave};
use hmi_gateway::GatewayService;
use hmi_transport_sim::{SimFactory, SimResponder};
use semver::Version;
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn write_pkg(data_dir: &Path) {
    let dir = data_dir.join("packages/com.test.meter/1.0.0");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        json!({
            "schemaVersion": 2,
            "id": "com.test.meter",
            "name": "测试电表",
            "version": "1.0.0",
            "protocol": { "driver": "modbus", "params": { "byteOrder": "abcd" } }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.join("points.json"),
        json!({
            "pollGroups": [{ "name": "fast", "intervalMs": 100 }],
            "points": [
                { "id": "uab", "name": "AB线电压", "address": "40101", "type": "u16",
                  "scale": 0.1, "unit": "V", "pollGroup": "fast" },
                { "id": "energy", "name": "电度", "address": "40200", "type": "u32",
                  "byteOrder": "cdab", "scale": 0.01, "unit": "kWh", "pollGroup": "fast" },
                { "id": "status", "name": "状态字", "address": "40110", "type": "u16",
                  "pollGroup": "fast", "bits": { "0": { "name": "运行" } } },
                { "id": "setpoint", "name": "设定值", "address": "40310", "type": "u16",
                  "scale": 0.1, "access": "rw", "min": 0, "max": 100, "step": 0.1,
                  "pollGroup": "fast" }
            ]
        })
        .to_string(),
    )
    .unwrap();
}

fn sim_channel() -> ChannelConfig {
    ChannelConfig {
        id: "sim1".into(),
        name: "测试总线".into(),
        timing: ChannelTiming {
            resp_timeout_ms: 100,
            retries: 1,
            probe_interval_ms: 150,
        },
        transport: TransportConfig::Sim(SimParams { seed: None }),
    }
}

fn device(id: &str, address: u32) -> DeviceConfig {
    DeviceConfig {
        id: id.into(),
        name: id.into(),
        channel_id: "sim1".into(),
        package: "com.test.meter".into(),
        address: address.into(),
        overrides: Default::default(),
    }
}

#[tokio::test]
async fn poll_write_and_offline_recovery() {
    let tmp = tempfile::tempdir().unwrap();
    write_pkg(tmp.path());

    let slave = Arc::new(
        SimSlave::new()
            .set_holding(100, 2201) // 40101 → 220.1 V
            .set_holding(199, 0x2710) // 40200 低字
            .set_holding(200, 0x0000) // 40200 高字（cdab）→ 10000 → 100.00 kWh
            .set_holding(109, 1) // 40110 状态字
            .set_holding(309, 250), // 40310 设定值
    );
    let offline9 = Arc::new(AtomicBool::new(true));
    let slave2 = slave.clone();
    let offline2 = offline9.clone();
    let responder: SimResponder = Arc::new(move |req: &[u8]| {
        if req.first() == Some(&9) && offline2.load(Ordering::Relaxed) {
            None
        } else {
            slave2.respond_rtu(req)
        }
    });
    let r2 = responder.clone();
    let factory = Arc::new(SimFactory::new(Arc::new(move |_ch: &ChannelConfig| {
        r2.clone()
    })));

    let drivers: Vec<Arc<dyn Driver>> = vec![Arc::new(ModbusDriver::new())];
    let svc =
        GatewayService::new(tmp.path(), Version::new(0, 1, 0), drivers, vec![factory]).unwrap();
    svc.upsert_channel(sim_channel()).await.unwrap();
    svc.add_device(device("pm1", 1)).await.unwrap();
    svc.add_device(device("pm9", 9)).await.unwrap();
    svc.start().await.unwrap();

    let mut ok = false;
    for _ in 0..100 {
        if svc
            .current_value("pm1.uab")
            .is_some_and(|v| v.quality == Quality::Good)
        {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(ok, "等待 pm1.uab good 超时");

    let v = svc.current_value("pm1.uab").unwrap().value;
    match v {
        Value::F64(f) => assert!((f - 220.1).abs() < 1e-9, "{f}"),
        other => panic!("期望 F64，得到 {other:?}"),
    }
    assert_eq!(
        svc.current_value("pm1.energy").unwrap().value,
        Value::F64(100.0)
    );
    assert_eq!(
        svc.current_value("pm1.status.b0").unwrap().value,
        Value::Bool(true)
    );

    // 坏设备离线，不阻塞好设备
    let mut pm9_offline = false;
    for _ in 0..100 {
        let snap = svc.snapshot().await;
        if snap
            .devices
            .iter()
            .find(|d| d.config.id == "pm9")
            .is_some_and(|d| !d.online)
        {
            pm9_offline = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(pm9_offline, "等待 pm9 offline 超时");
    assert!(svc.is_good("pm1.uab"), "坏设备不应阻塞好设备");

    // 写点：30.0 → 原始 300
    svc.write_tag("pm1.setpoint", Value::F64(30.0))
        .await
        .unwrap();
    assert_eq!(slave.holding_value(309), Some(300));

    // 步长校验
    assert!(svc
        .write_tag("pm1.setpoint", Value::F64(30.05))
        .await
        .is_err());
    // 只读点
    assert!(svc.write_tag("pm1.uab", Value::F64(1.0)).await.is_err());

    // 设备 9 恢复应答 → 探测恢复在线
    offline9.store(false, Ordering::Relaxed);
    let mut pm9_online = false;
    for _ in 0..100 {
        let snap = svc.snapshot().await;
        if snap
            .devices
            .iter()
            .find(|d| d.config.id == "pm9")
            .is_some_and(|d| d.online)
        {
            pm9_online = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(pm9_online, "等待 pm9 recovered 超时");
}
