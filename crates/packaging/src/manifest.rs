//! 设备包 manifest（manifest.json）模型。

use hmi_domain::points::ProtocolDefaults;
use serde::{Deserialize, Serialize};

/// 设备包清单：身份 + 版本兼容性 + 协议声明。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageManifest {
    /// 打点表结构版本（当前 1）
    pub schema_version: u32,
    /// 包 id，如 "com.acme.pm3000"（小写、点分）
    pub id: String,
    /// 显示名（可中文）
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    #[serde(default)]
    pub description: String,
    /// 包版本（semver）
    pub version: semver::Version,
    /// 最低宿主版本；缺省 = 不限制
    #[serde(default)]
    pub min_host_version: Option<semver::Version>,
    pub protocol: ProtocolDecl,
}

/// 协议声明。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolDecl {
    /// 驱动 id，如 "modbus-rtu"（存在性由宿主在安装/加载时校验）
    pub driver: String,
    /// 点表字段包级默认值
    #[serde(default)]
    pub defaults: ProtocolDefaults,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_manifest() {
        let j = r#"
        {
          "schemaVersion": 1,
          "id": "com.acme.pm3000",
          "name": "ACME PM3000 三相电表",
          "vendor": "ACME",
          "version": "1.2.0",
          "minHostVersion": "0.1.0",
          "protocol": {
            "driver": "modbus-rtu",
            "defaults": { "byteOrder": "cdab", "respTimeoutMs": 150 }
          }
        }"#;
        let m: PackageManifest = serde_json::from_str(j).unwrap();
        assert_eq!(m.id, "com.acme.pm3000");
        assert_eq!(m.version.to_string(), "1.2.0");
        assert_eq!(m.protocol.driver, "modbus-rtu");
        assert!(m.protocol.defaults.byte_order.is_some());
        assert_eq!(m.protocol.defaults.resp_timeout_ms, Some(150));
    }

    #[test]
    fn defaults_optional() {
        let j = r#"{ "schemaVersion":1, "id":"a.b", "name":"x", "version":"0.1.0",
                    "protocol": { "driver": "modbus-rtu" } }"#;
        let m: PackageManifest = serde_json::from_str(j).unwrap();
        assert_eq!(m.min_host_version, None);
        assert_eq!(m.protocol.defaults, ProtocolDefaults::default());
    }
}
