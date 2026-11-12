//! 架构回归：gateway 对协议零耦合。
//!
//! 一个完全非 Modbus 的 MockDriver（文本设备地址、结构化点地址、字符串值）
//! 必须能直接跑通调度/订阅/写点 —— 若 gateway 还残留寄存器/从站假设，此测试会失败。

use async_trait::async_trait;
use hmi_domain::config::{
    ChannelConfig, ChannelTiming, DeviceAddress, DeviceConfig, SimParams, TransportConfig,
};
use hmi_domain::driver::{
    CompileInput, DevicePlan, Driver, DriverError, PlanPayload, PollGroup, Sample,
};
use hmi_domain::link::{Link, TransportKind};
use hmi_domain::points::{PointDef, PointsTable};
use hmi_domain::protocol::ProtocolDecl;
use hmi_domain::value::{TagId, Value};
use hmi_gateway::GatewayService;
use hmi_transport_sim::SimFactory;
use semver::Version;
use serde_json::json;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct MockDriver {
    writes: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl Driver for MockDriver {
    fn id(&self) -> &'static str {
        "mock"
    }

    fn name(&self) -> &'static str {
        "Mock JSON"
    }

    fn transports(&self) -> &'static [TransportKind] {
        &[TransportKind::Sim]
    }

    fn validate_address(&self, address: &DeviceAddress) -> Result<(), String> {
        match address {
            DeviceAddress::Text(_) => Ok(()),
            _ => Err("mock 驱动需要文本地址".into()),
        }
    }

    fn compile(&self, input: &CompileInput<'_>) -> Result<DevicePlan, DriverError> {
        let tag = TagId::join(input.device_id, "x");
        Ok(DevicePlan {
            groups: vec![PollGroup {
                name: "default".into(),
                interval: Duration::from_millis(100),
                tags: vec![tag],
                payload: PlanPayload::new(()),
            }],
            warnings: Vec::new(),
        })
    }

    async fn poll(
        &self,
        _link: &dyn Link,
        _address: &DeviceAddress,
        group: &PollGroup,
        _timeout: Duration,
    ) -> Result<Vec<Sample>, DriverError> {
        Ok(vec![Sample::good(
            group.tags[0].clone(),
            Value::Str("hello".into()),
        )])
    }

    async fn write(
        &self,
        _link: &dyn Link,
        _address: &DeviceAddress,
        point: &PointDef,
        value: &Value,
        _protocol: &ProtocolDecl,
        _timeout: Duration,
    ) -> Result<(), DriverError> {
        self.writes
            .lock()
            .unwrap()
            .push(format!("{}={value}", point.id));
        Ok(())
    }

    async fn probe(
        &self,
        _link: &dyn Link,
        _address: &DeviceAddress,
        _protocol: &ProtocolDecl,
        _timeout: Duration,
    ) -> Result<(), DriverError> {
        Ok(())
    }
}

fn write_pkg(data_dir: &Path) {
    let dir = data_dir.join("packages/com.test.mock/1.0.0");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        json!({
            "schemaVersion": 2,
            "id": "com.test.mock",
            "name": "Mock 设备",
            "version": "1.0.0",
            "protocol": { "driver": "mock", "transports": ["sim"] }
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.join("points.json"),
        json!({
            "points": [
                { "id": "x", "name": "文本点", "address": { "path": "/x" }, "access": "rw" }
            ]
        })
        .to_string(),
    )
    .unwrap();
}

#[tokio::test]
async fn gateway_runs_non_modbus_driver() {
    let tmp = tempfile::tempdir().unwrap();
    write_pkg(tmp.path());

    let writes = Arc::new(Mutex::new(Vec::new()));
    let drivers: Vec<Arc<dyn Driver>> = vec![Arc::new(MockDriver {
        writes: writes.clone(),
    })];
    let transports: Vec<Arc<dyn hmi_domain::link::TransportFactory>> =
        vec![Arc::new(SimFactory::echo())];
    let svc = GatewayService::new(tmp.path(), Version::new(0, 1, 0), drivers, transports).unwrap();

    svc.upsert_channel(ChannelConfig {
        id: "sim1".into(),
        name: "mock".into(),
        timing: ChannelTiming {
            resp_timeout_ms: 100,
            retries: 1,
            probe_interval_ms: 150,
        },
        transport: TransportConfig::Sim(SimParams { seed: None }),
    })
    .await
    .unwrap();
    svc.add_device(DeviceConfig {
        id: "dev1".into(),
        name: "mock device".into(),
        channel_id: "sim1".into(),
        package: "com.test.mock".into(),
        address: "AA:BB:CC:DD:EE:FF".into(),
        overrides: Default::default(),
    })
    .await
    .unwrap();
    svc.start().await.unwrap();

    let mut got = false;
    for _ in 0..100 {
        if svc.current_value("dev1.x").is_some_and(|v| {
            v.quality == hmi_domain::value::Quality::Good && v.value == Value::Str("hello".into())
        }) {
            got = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(got, "等待 dev1.x 超时");

    svc.write_tag("dev1.x", Value::Str("set".into()))
        .await
        .unwrap();
    assert_eq!(writes.lock().unwrap().as_slice(), ["x=set"]);
}

#[test]
fn points_table_is_protocol_neutral() {
    // 结构化地址在 domain 层即可解析（不需要 Modbus）
    let t: PointsTable = serde_json::from_value(json!({
        "points": [{ "id": "x", "address": { "path": "/x" } }]
    }))
    .unwrap();
    assert!(t.point("x").unwrap().address.as_register().is_none());
}
