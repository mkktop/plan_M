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

/// 设备包声明的 UI（页面 + 自带 ESM 组件）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageUiDecl {
    #[serde(default)]
    pub pages: Vec<UiPageDecl>,
    /// 包自带的 ESM 小组件（入口在包目录 widgets/ 下，由 gatewayd 静态服务）。
    #[serde(default)]
    pub widgets: Vec<UiWidgetDecl>,
}

/// 包自带的 ESM 小组件声明。全局 kind = `<包id>:<id>`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiWidgetDecl {
    /// 包内短名（字母/数字/-/_，≤64）
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// 相对包目录的 ESM 入口，必须在 widgets/ 下
    pub entry: String,
    #[serde(default)]
    pub size: WidgetSizeDecl,
    #[serde(default)]
    pub bindings: BTreeMap<String, WidgetBindingDecl>,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// 组件默认尺寸与最小尺寸（12 列网格，单位 = 格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetSizeDecl {
    #[serde(default = "default_widget_w")]
    pub w: u32,
    #[serde(default = "default_widget_h")]
    pub h: u32,
    #[serde(default = "default_min_widget_w")]
    pub min_w: u32,
    #[serde(default = "default_min_widget_h")]
    pub min_h: u32,
}

impl Default for WidgetSizeDecl {
    fn default() -> Self {
        Self {
            w: default_widget_w(),
            h: default_widget_h(),
            min_w: default_min_widget_w(),
            min_h: default_min_widget_h(),
        }
    }
}

fn default_widget_w() -> u32 {
    6
}

fn default_widget_h() -> u32 {
    3
}

fn default_min_widget_w() -> u32 {
    2
}

fn default_min_widget_h() -> u32 {
    1
}

/// 槽位声明：宿主据此生成绑定编辑器并校验写权限。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetBindingDecl {
    /// point / value / text
    #[serde(rename = "type", default = "default_binding_type")]
    pub kind: String,
    #[serde(default)]
    pub many: bool,
    #[serde(default)]
    pub write: bool,
    #[serde(default)]
    pub required: bool,
}

fn default_binding_type() -> String {
    "point".into()
}

/// 一个设备页面。v1 用 `components[]`；v2 用 `schemaVersion: 2 + widgets[]`
/// （读取时翻译成 components，见 [`normalize_package_ui`]）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPageDecl {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: String,
    /// 页面结构版本：缺省/1 = components[]，2 = widgets[]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    /// v2 页面网格参数（宿主当前固定 12 列/72px/12px，读到仅做兼容校验）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<PageLayoutDecl>,
    /// v2 组件实例数组
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub widgets: Vec<UiWidgetInstanceDecl>,
    #[serde(default)]
    pub components: Vec<UiComponentDecl>,
}

/// v2 页面布局参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageLayoutDecl {
    #[serde(default = "default_cols")]
    pub cols: u32,
    #[serde(default = "default_row_height")]
    pub row_height: u32,
    #[serde(default = "default_gap")]
    pub gap: u32,
}

fn default_cols() -> u32 {
    12
}

fn default_row_height() -> u32 {
    72
}

fn default_gap() -> u32 {
    12
}

/// v2 页面里的一个组件实例。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiWidgetInstanceDecl {
    #[serde(default)]
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub bind: BTreeMap<String, BindingSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<WidgetRectDecl>,
    #[serde(default)]
    pub config: serde_json::Value,
}

/// 页面里的一个宿主组件。`config` 由组件自行解释，宿主原样保存。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiComponentDecl {
    /// v2 组件实例 id（v1 缺省）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub kind: String,
    #[serde(default)]
    pub title: String,
    /// 槽位 → 绑定声明。设备包只允许相对点绑定（点 id / 位号），
    /// 绝对 tag 绑定仅用户工作区页面可用。值为单声明或声明数组（多点槽位）。
    #[serde(default)]
    pub bind: BTreeMap<String, BindingSpec>,
    /// 网格位置（12 列）；整页要么全部声明、要么全部省略（流式）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<WidgetRectDecl>,
    #[serde(default)]
    pub config: serde_json::Value,
}

/// 一个槽位的绑定：单绑定或绑定数组。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BindingSpec {
    One(BindingDecl),
    Many(Vec<BindingDecl>),
}

impl Default for BindingSpec {
    fn default() -> Self {
        Self::One(BindingDecl::default())
    }
}

impl BindingSpec {
    pub fn decls(&self) -> &[BindingDecl] {
        match self {
            Self::One(d) => std::slice::from_ref(d),
            Self::Many(v) => v,
        }
    }
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

impl PackageUiDecl {
    /// 读取时归一化（不写回用户包）：
    ///
    /// - `schemaVersion: 2` 页面的 `widgets[]` → 内存里的 `components[]`（保留实例 id）；
    /// - 页面里引用包内组件短名（如 `trend`）→ 全局 kind `<包id>:trend`。
    pub fn normalize(&mut self, package_id: &str) {
        let local: Vec<&str> = self.widgets.iter().map(|w| w.id.as_str()).collect();
        for page in &mut self.pages {
            // 只在「纯 v2」页面上翻译；components/widgets 同时存在视为作者混用，
            // 保留原样交给校验报错（避免静默丢弃一半）。
            if page.schema_version.unwrap_or(1) >= 2
                && !page.widgets.is_empty()
                && page.components.is_empty()
            {
                let widgets = std::mem::take(&mut page.widgets);
                page.components = widgets
                    .into_iter()
                    .map(|w| UiComponentDecl {
                        id: if w.id.is_empty() { None } else { Some(w.id) },
                        kind: w.kind,
                        title: w.title,
                        bind: w.bind,
                        rect: w.rect,
                        config: w.config,
                    })
                    .collect();
            }
            for comp in &mut page.components {
                if local.contains(&comp.kind.as_str()) {
                    comp.kind = format!("{package_id}:{}", comp.kind);
                }
            }
        }
    }
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

    /// P3/P4：v2 页面（widgets[]）与包内 ESM 组件声明可解析，并可归一化到 components。
    #[test]
    fn parses_v2_pages_and_widgets() {
        let j = r#"
        {
          "schemaVersion": 1, "id": "com.acme.pm3000", "name": "PM3000",
          "version": "1.0.0", "protocol": { "driver": "modbus-rtu" },
          "ui": {
            "widgets": [
              { "id": "trend", "title": "趋势图", "entry": "widgets/trend.js",
                "size": { "w": 6, "h": 3, "minW": 3, "minH": 2 },
                "bindings": { "series": { "type": "point", "many": true } },
                "permissions": ["tags:read"] }
            ],
            "pages": [
              { "schemaVersion": 2, "id": "monitor", "title": "监测",
                "layout": { "cols": 12, "rowHeight": 72, "gap": 12 },
                "widgets": [
                  { "id": "w1", "kind": "core:value",
                    "rect": { "x": 0, "y": 0, "w": 3, "h": 2 },
                    "bind": { "value": { "point": "uab" } } },
                  { "id": "w2", "kind": "trend",
                    "rect": { "x": 3, "y": 0, "w": 6, "h": 3 },
                    "bind": { "series": [ { "point": "uab" }, { "point": "ia" } ] } }
                ] }
            ]
          }
        }"#;
        let mut m: PackageManifest = serde_json::from_str(j).unwrap();
        assert_eq!(m.ui.widgets.len(), 1);
        m.ui.normalize(&m.id);
        let page = &m.ui.pages[0];
        assert_eq!(page.components.len(), 2);
        assert_eq!(page.components[0].id.as_deref(), Some("w1"));
        assert_eq!(page.components[1].kind, "com.acme.pm3000:trend");
        assert_eq!(page.components[1].bind["series"].decls().len(), 2);
    }
}
