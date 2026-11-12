//! 设备包里的协议声明。
//!
//! `driver` 是宿主驱动注册表的键；`transports` 可选地限制该包能跑在哪些
//! 传输上（缺省 = 驱动支持的全部）；`params` 是驱动私有的默认参数，
//! domain 不解释其形状，由驱动在安装校验/编译时解析。

use crate::link::TransportKind;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolDecl {
    /// 驱动 id，如 `"modbus"` / `"ble-gatt"` / `"script"`
    pub driver: String,
    /// 该包允许的传输；空 = 驱动支持的全部
    #[serde(default)]
    pub transports: Vec<TransportKind>,
    /// 驱动私有默认参数（如 Modbus 的 framing / byteOrder）
    #[serde(default = "empty_object")]
    pub params: Value,
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

impl ProtocolDecl {
    /// 解析驱动私有参数；类型不符返回可读错误。
    pub fn params_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, String> {
        serde_json::from_value(self.params.clone())
            .map_err(|e| format!("protocol.params 不合法: {e}"))
    }

    /// 该包在指定传输上是否可用（考虑驱动能力）。
    pub fn allows_transport(&self, kind: TransportKind, driver_supports: &[TransportKind]) -> bool {
        let driver_ok = driver_supports.contains(&kind);
        let package_ok = self.transports.is_empty() || self.transports.contains(&kind);
        driver_ok && package_ok
    }
}

/// 地址扫描范围（u32 以覆盖 Modbus 1–247 及未来的更大地址空间）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRange {
    pub from: u32,
    pub to: u32,
}

/// 驱动对设备包的校验结果。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DriverIssues {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl DriverIssues {
    pub fn error(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }

    pub fn warning(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }

    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}
