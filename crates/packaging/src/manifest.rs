//! 设备包 manifest（manifest.json）模型。

use hmi_domain::points::ProtocolDefaults;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// 设备类型工作区默认页面（可选；旧包缺省时宿主使用内置四页）。
    #[serde(default)]
    pub ui: PackageUiDecl,
}

/// 设备包声明的 UI（当前只有页面贡献点）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageUiDecl {
    #[serde(default)]
    pub pages: Vec<UiPageDecl>,
}

/// 一个设备页面：宿主按 `components` 顺序渲染。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPageDecl {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub components: Vec<UiComponentDecl>,
}

/// 页面里的一个宿主组件。`config` 由组件自行解释，宿主原样保存。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiComponentDecl {
    pub kind: String,
    #[serde(default)]
    pub title: String,
    /// 槽位 → 绑定声明。设备包只允许相对点绑定（点 id / 位号），
    /// 绝对 tag 绑定仅用户工作区页面可用。
    #[serde(default)]
    pub bind: BTreeMap<String, BindingDecl>,
    /// 网格位置（12 列）；整页要么全部声明、要么全部省略（流式）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<WidgetRectDecl>,
    #[serde(default)]
    pub config: serde_json::Value,
}

/// 组件在 12 列页面网格中的位置（单位 = 格；行高由宿主决定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetRectDecl {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// 一个槽位的绑定：`point`（相对点）或 `tag`（绝对 tag）二选一；
/// `bit` 只能与 `point` 组合。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingDecl {
    /// 包内相对点 id（渲染时由宿主解析为 `<设备实例id>.<point>`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<String>,
    /// 位号（对应点 `bits` 里的定义），解析为 `<设备实例id>.<point>.b<bit>`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit: Option<u8>,
    /// 绝对 tag（如 `pm1.uab`），设备包中禁止使用
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
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
        assert!(m.ui.pages.is_empty());
    }

    #[test]
    fn parses_ui_pages() {
        let j = r#"
        {
          "schemaVersion": 1, "id": "com.acme.radar", "name": "雷达物位计",
          "version": "1.0.0", "protocol": { "driver": "modbus-rtu" },
          "ui": {
            "pages": [
              { "id": "overview", "title": "设备概览", "icon": "pulse",
                "components": [{ "kind": "overview", "title": "运行状态" }] },
              { "id": "dashboard", "title": "仪表盘", "icon": "layout",
                "components": [{ "kind": "dashboard" }, { "kind": "tags" }] }
            ]
          }
        }"#;
        let m: PackageManifest = serde_json::from_str(j).unwrap();
        assert_eq!(m.ui.pages.len(), 2);
        assert_eq!(m.ui.pages[0].components[0].kind, "overview");
        assert_eq!(m.ui.pages[1].components.len(), 2);
    }
}
