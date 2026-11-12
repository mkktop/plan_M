//! 端到端集成测试：虚拟 485 总线上的完整采集链路。
//!
//! 覆盖：编译 → 通道监督器 → 事务串行 → 解码 → TagHub 值/质量流转 →
//! 从站离线/恢复 → 坏设备不阻塞同总线好设备 → 写命令。

use hmi_domain::config::{DeviceConfig, Parity, ProjectConfig, SerialChannelConfig};
use hmi_domain::points::{Access, BitDef, PointDef, PointsTable, PollGroupDef, ProtocolDefaults};
use hmi_domain::value::{Quality, TagId, Value};
use hmi_driver_modbus_rtu::{sim_pair, spawn_slave_endpoint, ModbusRtuDriver, SimSlave};
use hmi_gateway::poll::compile;
use hmi_gateway::supervisor::{
    spawn_channel, BusOpener, ChannelRuntime, DeviceRuntime, GatewayEvent,
};
use hmi_gateway::{ConfigStore, TagHub};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn pm_like_table() -> PointsTable {
    let mut bits = BTreeMap::new();
    bits.insert(
        0,
        BitDef {
            name: "运行".into(),
            on: None,
            off: None,
        },
    );
    PointsTable {
        poll_groups: vec![
            PollGroupDef {
                name: "fast".into(),
                interval_ms: 100,
            },
            PollGroupDef {
                name: "slow".into(),
                interval_ms: 300,
            },
        ],
        points: vec![
            PointDef {
                id: "uab".into(),
                name: "AB线电压".into(),
                register: "40101".parse().unwrap(),
                data_type: hmi_domain::codec::DataType::U16,
                byte_order: None,
                scale: Some(0.1),
                offset: None,
                unit: "V".into(),
                access: Access::R,
                poll_group: Some("fast".into()),
                bits: BTreeMap::new(),
                min: None,
                max: None,
                step: None,
                value_enum: Vec::new(),
            },
            PointDef {
                id: "energy".into(),
                name: "有功电度".into(),
                register: "40200".parse().unwrap(),
                data_type: hmi_domain::codec::DataType::U32,
                byte_order: Some(hmi_domain::codec::ByteOrder::Cdab),
                scale: Some(0.01),
                offset: None,
                unit: "kWh".into(),
                access: Access::R,
                poll_group: Some("slow".into()),
                bits: BTreeMap::new(),
                min: None,
                max: None,
                step: None,
                value_enum: Vec::new(),
            },
            PointDef {
                id: "status".into(),
                name: "状态字".into(),
                register: "40110".parse().unwrap(),
                data_type: hmi_domain::codec::DataType::U16,
                byte_order: None,
                scale: None,
                offset: None,
                unit: String::new(),
                access: Access::R,
                poll_group: Some("fast".into()),
                bits,
                min: None,
                max: None,
                step: None,
                value_enum: Vec::new(),
            },
            PointDef {
                id: "setpoint".into(),
                name: "设定值".into(),
                register: "40300".parse().unwrap(),
                data_type: hmi_domain::codec::DataType::U16,
                byte_order: None,
                scale: None,
                offset: None,
                unit: String::new(),
                access: Access::Rw,
                poll_group: None,
                bits: BTreeMap::new(),
                min: None,
                max: None,
                step: None,
                value_enum: Vec::new(),
            },
        ],
    }
}

fn channel_cfg() -> SerialChannelConfig {
    SerialChannelConfig {
        id: "ch1".into(),
        name: "测试总线".into(),
        port: "SIM".into(),
        hw_id: None,
        baud_rate: 9600,
        data_bits: 8,
        parity: Parity::None,
        stop_bits: 1,
        resp_timeout_ms: 120,
        retries: 2,
        inter_frame_gap_ms: 1,
    }
}

fn device_cfg(id: &str, slave: u8) -> DeviceConfig {
    DeviceConfig {
        id: id.into(),
        name: String::new(),
        channel_id: "ch1".into(),
        package: "test.pkg".into(),
        slave,
        overrides: Default::default(),
    }
}

/// 组装一条"共享虚拟总线"通道（两个设备挂同一总线）。
async fn start_channel() -> (
    Arc<TagHub>,
    tokio::sync::broadcast::Receiver<GatewayEvent>,
    Arc<SimSlave>,
    hmi_gateway::ChannelHandle,
) {
    let (transport, endpoint) = sim_pair();
    let slave = Arc::new(
        SimSlave::new()
            .set_holding(100, 2200) // uab → 220.0
            .set_holding(109, 0x2001) // 40110 状态字：b0=1, b13=1
            .set_holding(199, 0x0064) // energy cdab: 低字 100
            .set_holding(200, 0x2710), // energy cdab: 高字 10000 → u32=0x27100064 → *0.01
    );
    spawn_slave_endpoint(endpoint, slave.clone());

    let hub = Arc::new(TagHub::new());
    let (events_tx, events_rx) = tokio::sync::broadcast::channel(64);

    let driver = Arc::new(ModbusRtuDriver::new());
    let table = pm_like_table();
    let mut devices = Vec::new();
    for (id, slave_addr) in [("pm1", 1u8), ("pm2", 2u8)] {
        let cfg = device_cfg(id, slave_addr);
        let plan = compile(id, &cfg.overrides, &table, &ProtocolDefaults::default()).unwrap();
        let all_tags = plan.groups.iter().flat_map(|g| g.all_tags()).collect();
        devices.push(DeviceRuntime {
            device_id: id.into(),
            slave: slave_addr,
            driver: driver.clone(),
            plan,
            all_tags,
        });
    }

    // opener 每次返回同一共享总线（SIM 语义）
    let shared: Arc<dyn hmi_domain::transport::BusTransport> = Arc::new(transport);
    let opener: BusOpener =
        Arc::new(move |_cfg| Ok(Box::new(hmi_gateway::supervisor::SharedBus(shared.clone()))));

    let rt = ChannelRuntime {
        cfg: channel_cfg(),
        devices,
        open_bus: opener,
        hub: hub.clone(),
        events: events_tx,
    };
    let handle = spawn_channel(rt, 32);
    (hub, events_rx, slave, handle)
}

/// 轮询 hub 直到 tag 满足条件或超时。
async fn wait_tag(
    hub: &TagHub,
    tag: &str,
    pred: impl Fn(&hmi_domain::value::TagSnapshot) -> bool,
) -> hmi_domain::value::TagSnapshot {
    let id = TagId::new(tag);
    for _ in 0..200 {
        if let Some(s) = hub.get(&id) {
            if pred(&s) {
                return s;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("等待 tag {tag} 超时，当前: {:?}", hub.get(&id));
}

#[tokio::test]
async fn full_loop_values_and_quality() {
    let (hub, _events, _slave, _handle) = start_channel().await;

    // 快组：电压缩放
    let s = wait_tag(&hub, "pm1.uab", |s| s.quality == Quality::Good).await;
    assert_eq!(s.value, Value::F64(220.0));

    // 状态字位展开
    let s = wait_tag(&hub, "pm1.status.b0", |s| s.quality == Quality::Good).await;
    assert_eq!(s.value, Value::Bool(true));

    // 慢组：u32 cdab 字节序 + 缩放
    // words = [0x0064, 0x2710]（40200 低字在前）→ cdab → u32 0x27100064 = 655360100 → *0.01
    let s = wait_tag(&hub, "pm1.energy", |s| s.quality == Quality::Good).await;
    assert_eq!(s.value, Value::F64(655360100.0 * 0.01));

    // 第二台设备同样在采
    let s = wait_tag(&hub, "pm2.uab", |s| s.quality == Quality::Good).await;
    assert_eq!(s.value, Value::F64(220.0));
}

#[tokio::test]
async fn comm_lost_and_recovery() {
    let (hub, mut events, slave, _handle) = start_channel().await;
    wait_tag(&hub, "pm1.uab", |s| s.quality == Quality::Good).await;

    // 掉线（不应答）→ 重试耗尽 → CommLost
    slave.set_online(false);
    let s = wait_tag(&hub, "pm1.uab", |s| s.quality == Quality::CommLost).await;
    // 保留最后好值
    assert_eq!(s.value, Value::F64(220.0));

    // 收到设备离线事件
    let mut saw_offline = false;
    while let Ok(ev) = events.try_recv() {
        if let GatewayEvent::DeviceOnline { online: false, .. } = ev {
            saw_offline = true;
        }
    }
    assert!(saw_offline, "应收到 DeviceOnline(false) 事件");

    // 恢复 → 探测成功 → 回 Good
    slave.set_online(true);
    let s = wait_tag(&hub, "pm1.uab", |s| s.quality == Quality::Good).await;
    assert_eq!(s.value, Value::F64(220.0));
}

#[tokio::test]
async fn bad_slave_does_not_block_good_one() {
    // 自定义应答器：从站 2 掉线（不应答），从站 1 正常
    let (transport, endpoint) = sim_pair();
    let slave = Arc::new(SimSlave::new().set_holding(100, 2200));
    let pm2_alive = Arc::new(AtomicBool::new(true));
    let flag = pm2_alive.clone();
    tokio::spawn(async move {
        endpoint
            .run(move |req| {
                if req[0] == 2 && !flag.load(Ordering::Relaxed) {
                    return None; // 从站 2 不应答
                }
                slave.respond(req)
            })
            .await;
    });

    let hub = Arc::new(TagHub::new());
    let (events_tx, _events_rx) = tokio::sync::broadcast::channel(64);
    let driver = Arc::new(ModbusRtuDriver::new());
    let table = pm_like_table();
    let mut devices = Vec::new();
    for (id, slave_addr) in [("pm1", 1u8), ("pm2", 2u8)] {
        let plan = compile(
            id,
            &Default::default(),
            &table,
            &ProtocolDefaults::default(),
        )
        .unwrap();
        let all_tags = plan.groups.iter().flat_map(|g| g.all_tags()).collect();
        devices.push(DeviceRuntime {
            device_id: id.into(),
            slave: slave_addr,
            driver: driver.clone(),
            plan,
            all_tags,
        });
    }
    let shared: Arc<dyn hmi_domain::transport::BusTransport> = Arc::new(transport);
    let opener: BusOpener =
        Arc::new(move |_cfg| Ok(Box::new(hmi_gateway::supervisor::SharedBus(shared.clone()))));
    let handle = spawn_channel(
        ChannelRuntime {
            cfg: channel_cfg(),
            devices,
            open_bus: opener,
            hub: hub.clone(),
            events: events_tx,
        },
        32,
    );
    let _ = handle;

    wait_tag(&hub, "pm1.uab", |s| s.quality == Quality::Good).await;
    wait_tag(&hub, "pm2.uab", |s| s.quality == Quality::Good).await;

    // pm2 掉线
    pm2_alive.store(false, Ordering::Relaxed);
    wait_tag(&hub, "pm2.uab", |s| s.quality == Quality::CommLost).await;

    // pm1 持续刷新：时间戳在 800ms 观察窗内持续推进（fast 组 100ms 周期）
    let mut last_ts = 0u64;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(800);
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(120)).await;
        if let Some(s) = hub.get(&TagId::new("pm1.uab")) {
            assert_eq!(s.quality, Quality::Good, "好设备不应被坏设备拖下水");
            assert!(s.ts_ms >= last_ts, "pm1 时间戳应推进");
            last_ts = s.ts_ms;
        }
    }
    assert!(last_ts > 0, "pm1 应持续有新数据");
}

#[tokio::test]
async fn write_via_channel_cmd() {
    let (hub, _events, slave, handle) = start_channel().await;
    wait_tag(&hub, "pm1.uab", |s| s.quality == Quality::Good).await;

    let driver = Arc::new(ModbusRtuDriver::new());
    handle
        .write_regs(driver, 1, 299, vec![1234]) // 40300 → offset 299
        .await
        .unwrap();
    assert_eq!(slave.holding_value(299), Some(1234));

    // 轮询把新值带回来
    let s = wait_tag(&hub, "pm1.setpoint", |s| {
        s.quality == Quality::Good && s.value == Value::I64(1234)
    })
    .await;
    assert_eq!(s.value, Value::I64(1234));
}

#[tokio::test]
async fn config_store_roundtrip() {
    use hmi_gateway::store::JsonConfigStore;
    let dir = tempfile::tempdir().unwrap();
    let store = JsonConfigStore::new(dir.path().join("config.json"));
    let mut cfg = ProjectConfig::default();
    cfg.channels.push(channel_cfg());
    cfg.devices.push(device_cfg("pm1", 1));
    store.save(&cfg).unwrap();
    let back = store.load().unwrap();
    assert_eq!(back.channels.len(), 1);
    assert_eq!(back.devices[0].slave, 1);
}
