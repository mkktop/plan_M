//! Modbus TCP 端到端：真实 TcpListener + MBAP 从站，验证第二个传输/帧封装
//! 与 485 路径共用同一驱动、同一设备包、同一调度器。

use hmi_domain::config::{ChannelConfig, ChannelTiming, DeviceConfig, TcpParams, TransportConfig};
use hmi_domain::driver::Driver;
use hmi_domain::value::{Quality, Value};
use hmi_driver_modbus::{serve_tcp_stream, ModbusDriver, SimSlave};
use hmi_gateway::GatewayService;
use semver::Version;
use serde_json::json;
use std::path::Path;
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
            "protocol": {
                "driver": "modbus",
                "transports": ["serial", "tcp"],
                "params": { "framing": "auto", "byteOrder": "abcd" }
            }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.join("points.json"),
        json!({
            "pollGroups": [{ "name": "fast", "intervalMs": 100 }],
            "points": [
                { "id": "uab", "name": "电压", "address": "40101", "type": "u16",
                  "scale": 0.1, "unit": "V", "pollGroup": "fast" },
                { "id": "setpoint", "name": "设定值", "address": "40310", "type": "u16",
                  "scale": 0.1, "access": "rw", "min": 0, "max": 100, "step": 0.1,
                  "pollGroup": "fast" }
            ]
        })
        .to_string(),
    )
    .unwrap();
}

#[tokio::test]
async fn modbus_tcp_end_to_end() {
    let tmp = tempfile::tempdir().unwrap();
    write_pkg(tmp.path());

    // 本地 Modbus TCP 从站
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let slave = Arc::new(SimSlave::new().set_holding(100, 2201).set_holding(309, 250));
    let s2 = slave.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let slave = s2.clone();
            tokio::spawn(async move {
                let _ = serve_tcp_stream(stream, slave).await;
            });
        }
    });

    let drivers: Vec<Arc<dyn Driver>> = vec![Arc::new(ModbusDriver::new())];
    let transports: Vec<Arc<dyn hmi_domain::link::TransportFactory>> =
        vec![Arc::new(hmi_transport_tcp::TcpFactory)];
    let svc = GatewayService::new(tmp.path(), Version::new(0, 1, 0), drivers, transports).unwrap();

    svc.upsert_channel(ChannelConfig {
        id: "tcp1".into(),
        name: "TCP 网关".into(),
        timing: ChannelTiming {
            resp_timeout_ms: 300,
            retries: 1,
            probe_interval_ms: 300,
        },
        transport: TransportConfig::Tcp(TcpParams {
            host: "127.0.0.1".into(),
            port: addr.port(),
            connect_timeout_ms: 1000,
        }),
    })
    .await
    .unwrap();
    svc.add_device(DeviceConfig {
        id: "pm1".into(),
        name: "TCP 电表".into(),
        channel_id: "tcp1".into(),
        package: "com.test.meter".into(),
        address: 1.into(),
        overrides: Default::default(),
    })
    .await
    .unwrap();
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
    assert!(ok, "等待 TCP pm1.uab good 超时");
    match svc.current_value("pm1.uab").unwrap().value {
        Value::F64(f) => assert!((f - 220.1).abs() < 1e-9, "{f}"),
        other => panic!("期望 F64，得到 {other:?}"),
    }

    svc.write_tag("pm1.setpoint", Value::F64(30.0))
        .await
        .unwrap();
    assert_eq!(slave.holding_value(309), Some(300));
}
