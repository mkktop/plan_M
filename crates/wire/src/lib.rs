//! gatewayd ↔ UI 的 WebSocket 消息契约。
//!
//! 传输形态：文本帧，内容为 `serde_json` 序列化的 [`ClientMsg`] / [`ServerMsg`]，
//! 判别字段 `type`（camelCase）。
//!
//! 握手：连接后客户端第一条消息必须是 [`ClientMsg::Hello`]（携带 token），
//! 服务端回 [`ServerMsg::Welcome`]；token 错误服务端直接断开。
//!
//! 订阅制：UI 只订阅当前画面用到的 tag；`Changes` 为 10~20ms 合并后的增量批量。

use hmi_domain::addr::RegArea;
use hmi_domain::codec::DataType;
use hmi_domain::config::{ChannelConfig, DeviceAddress, DeviceConfig};
use hmi_domain::link::{TransportKind, TransportTarget};
use hmi_domain::points::{Access, PointAddress};
use hmi_domain::protocol::{ProtocolDecl, ScanRange};
use hmi_domain::value::{TagChange, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// WS 契约版本。UI 用它判断是否支持宿主下发的消息形状。
pub const API_VERSION: u32 = 2;

// —— 客户端 → 服务端 ——

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ClientMsg {
    /// 握手
    Hello {
        token: String,
    },
    /// 全量快照（通道/设备/包/全部当前值）
    GetSnapshot,
    /// 订阅 tag（增量合并推送）
    Subscribe {
        tags: Vec<String>,
    },
    /// 退订 tag
    Unsubscribe {
        tags: Vec<String>,
    },
    /// 写点
    WriteTag {
        tag: String,
        value: Value,
    },
    /// 扫描通道上的设备地址；`driver` 缺省时由宿主按通道选择唯一驱动
    ScanChannel {
        channel: String,
        #[serde(default)]
        driver: Option<String>,
        range: Option<ScanRange>,
    },
    /// 枚举某种传输的可发现目标（串口列表等）
    GetTransportTargets {
        transport: TransportKind,
    },
    /// 已安装设备包清单
    GetPackages,
    /// 新增/更新通道
    UpsertChannel {
        config: ChannelConfig,
    },
    RemoveChannel {
        channel: String,
    },
    /// 新增/更新设备实例
    AddDevice {
        config: DeviceConfig,
    },
    RemoveDevice {
        device: String,
    },
    /// 导入设备包（gatewayd 本机路径，目录或 zip）
    ImportPackage {
        path: String,
    },
    /// 删除设备包（该 id 的全部版本）；有设备在用时拒绝
    RemovePackage {
        id: String,
    },
    /// 仪表盘布局（工程数据，持久化在 gatewayd）
    GetDashboards,
    /// 新建/保存仪表盘（同 id 即更新）
    SaveDashboard {
        dashboard: DashboardDto,
    },
    RemoveDashboard {
        id: String,
    },
    /// 已安装的 UI 插件清单
    GetUiPlugins,
    /// 导入 UI 插件（gatewayd 本机路径，目录或 zip）
    ImportUiPlugin {
        path: String,
    },
    RemoveUiPlugin {
        id: String,
    },
    /// 设备类型工作区页面（用户覆盖包默认页面）
    GetWorkspaces,
    /// 保存某个设备类型的页面配置（同 deviceType 即覆盖）
    SaveWorkspace {
        workspace: WorkspaceDto,
    },
    /// 删除某个类型的页面覆盖，回落到设备包默认页面
    RemoveWorkspace {
        device_type: String,
    },
    /// 包编辑器：校验草稿（manifest / points 由 UI 表单生成，不落盘）
    ValidatePackage {
        manifest: serde_json::Value,
        points: serde_json::Value,
    },
    /// 包编辑器：安装草稿（完整安装流校验后落到 gatewayd 数据目录）
    InstallDraftPackage {
        manifest: serde_json::Value,
        points: serde_json::Value,
    },
    /// 包编辑器：导出草稿到本机目录（写 manifest.json + points.json）
    ExportPackage {
        manifest: serde_json::Value,
        points: serde_json::Value,
        dir: String,
    },
}

// —— 服务端 → 客户端 ——

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ServerMsg {
    /// 握手应答：宿主版本 + 能力（UI 据此显示传输/驱动选项）
    Welcome {
        host_version: String,
        api_version: u32,
        transports: Vec<TransportKind>,
        drivers: Vec<DriverInfoDto>,
    },
    Snapshot(SnapshotData),
    /// tag 增量批量
    Changes {
        items: Vec<TagChange>,
    },
    /// 通道状态变化：running / recovering / faulted / stopped
    ChannelState {
        channel: String,
        state: String,
        detail: String,
    },
    /// 设备上下线（与通道状态分开，UI 不必从 channelState 猜）
    DeviceState {
        device: String,
        online: bool,
    },
    /// 扫描进度
    ScanProgress {
        channel: String,
        address: DeviceAddress,
        found: bool,
    },
    ScanDone {
        channel: String,
        found: Vec<DeviceAddress>,
    },
    TransportTargets {
        transport: TransportKind,
        targets: Vec<TransportTarget>,
    },
    Packages {
        packages: Vec<PackageDto>,
    },
    /// 仪表盘列表（get/save/remove 的统一应答）
    Dashboards {
        dashboards: Vec<DashboardDto>,
    },
    /// UI 插件列表（get/import/remove 的统一应答）
    UiPlugins {
        plugins: Vec<UiPluginDto>,
    },
    /// 设备类型工作区列表（get/save/remove 的统一应答）
    Workspaces {
        workspaces: Vec<WorkspaceDto>,
    },
    /// 包草稿校验结果（errors 非空 = 不能安装）
    PackageValidation {
        errors: Vec<String>,
        warnings: Vec<String>,
    },
    /// 命令成功（kind 与请求对应，如 "writeTag"）
    CommandOk {
        kind: String,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotData {
    pub channels: Vec<ChannelDto>,
    pub devices: Vec<DeviceDto>,
    pub packages: Vec<PackageDto>,
    /// 全部 tag 当前值（含质量戳）
    pub tags: Vec<TagChange>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDto {
    #[serde(flatten)]
    pub config: ChannelConfig,
    /// running / recovering / faulted / stopped
    pub state: String,
    /// 传输未注册/配置错误等（None = 正常）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    #[serde(flatten)]
    pub config: DeviceConfig,
    pub online: bool,
    /// 驱动缺失/编译失败等配置错误（None = 正常）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDto {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub protocol: ProtocolDecl,
    #[serde(default)]
    pub description: String,
    /// 设备包声明的默认工作区页面（空 = 宿主 fallback 四页）
    #[serde(default)]
    pub ui: PackageUiDto,
    /// 点位定义，供 UI 做点表展示、写值入口与 tag 名映射
    pub points: Vec<PointDto>,
}

/// 驱动能力（Welcome 下发，UI 据此过滤设备包与表单）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverInfoDto {
    pub id: String,
    pub name: String,
    pub transports: Vec<TransportKind>,
}

/// 设备包 UI 声明。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageUiDto {
    #[serde(default)]
    pub pages: Vec<DevicePageDto>,
    /// 设备包自带的 ESM 小组件（P3）；页面里以 `<包id>:<组件id>` 引用。
    #[serde(default)]
    pub widgets: Vec<PackageWidgetDto>,
}

/// 设备包自带的 ESM 小组件声明。
///
/// 入口文件由 gatewayd 经 `/packages/<id>/<version>/<entry>` 静态提供，
/// 只暴露声明 entry 所在目录下的文件。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageWidgetDto {
    /// 包内短名（全局 kind = `<包id>:<id>`）
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// 包目录内的相对路径（必须在 widgets/ 下）
    pub entry: String,
    #[serde(default)]
    pub size: WidgetSizeDto,
    /// 槽位声明：宿主据此生成绑定编辑器、校验写权限
    #[serde(default)]
    pub bindings: BTreeMap<String, WidgetBindingDto>,
    /// 配置 schema / 默认值（宿主原样透传给组件）
    #[serde(default)]
    pub config: serde_json::Value,
    /// tags:read / tags:write / notify
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// 组件默认尺寸（12 列网格，单位 = 格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetSizeDto {
    #[serde(default = "default_widget_w")]
    pub w: u32,
    #[serde(default = "default_widget_h")]
    pub h: u32,
    #[serde(default = "default_min_widget_w")]
    pub min_w: u32,
    #[serde(default = "default_min_widget_h")]
    pub min_h: u32,
}

impl Default for WidgetSizeDto {
    fn default() -> Self {
        Self {
            w: default_widget_w(),
            h: default_widget_h(),
            min_w: default_min_widget_w(),
            min_h: default_min_widget_h(),
        }
    }
}

fn default_min_widget_w() -> u32 {
    2
}

fn default_min_widget_h() -> u32 {
    1
}

/// 组件槽位声明。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetBindingDto {
    /// point / value / text
    #[serde(rename = "type", default = "default_binding_type")]
    pub kind: String,
    /// 一个槽位可绑定多个点（趋势图等）
    #[serde(default)]
    pub many: bool,
    /// 组件可经 `host.slots.write` 写入该槽位
    #[serde(default)]
    pub write: bool,
    #[serde(default)]
    pub required: bool,
}

fn default_binding_type() -> String {
    "point".into()
}

/// 设备工作区的一页：由宿主组件按顺序组合。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePageDto {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub components: Vec<DevicePageComponentDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePageComponentDto {
    /// v2 组件实例 id（同页内稳定，便于将来增量更新）；v1 组件为空
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// 宿主组件：overview / dashboard / tags / settings / core:value ... /
    /// `plugin:<插件id>:<组件id>` / `<包id>:<组件短名>`
    pub kind: String,
    #[serde(default)]
    pub title: String,
    /// 槽位 → 绑定；包内通常声明相对点（point/bit），用户页面可声明绝对 tag。
    /// 值为单个声明或声明数组（`many: true` 的多点槽位）。
    #[serde(default)]
    pub bind: BTreeMap<String, BindingSpecDto>,
    /// 12 列网格位置；整页要么全部声明、要么全部省略（流式）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<WidgetRectDto>,
    #[serde(default)]
    pub config: serde_json::Value,
}

/// 一个槽位的绑定规格：单绑定或绑定数组（多点槽位）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BindingSpecDto {
    One(BindingDeclDto),
    Many(Vec<BindingDeclDto>),
}

impl Default for BindingSpecDto {
    fn default() -> Self {
        Self::One(BindingDeclDto::default())
    }
}

impl BindingSpecDto {
    /// 展开为声明列表（单绑定的临时 Vec 借用不方便，统一返回切片）。
    pub fn decls(&self) -> &[BindingDeclDto] {
        match self {
            Self::One(d) => std::slice::from_ref(d),
            Self::Many(v) => v,
        }
    }
}

/// 组件网格位置（0 基，单位 = 格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetRectDto {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// 一个槽位的绑定声明：point 与 tag 二选一，bit 只与 point 组合。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingDeclDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

/// 某个设备类型的工作区页面配置（workspaces.json）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDto {
    /// 设备类型 = 设备包 id
    pub device_type: String,
    #[serde(default)]
    pub pages: Vec<DevicePageDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointDto {
    pub id: String,
    pub name: String,
    /// 协议地址：寄存器字符串或驱动私有对象
    pub address: PointAddress,
    /// 寄存器类协议的显示区（非寄存器协议为 None）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<RegArea>,
    #[serde(rename = "type")]
    pub data_type: DataType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_order: Option<hmi_domain::codec::ByteOrder>,
    pub access: Access,
    pub scale: Option<f64>,
    pub offset: Option<f64>,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_group: Option<String>,
    /// 状态字位展开定义（位号升序；JSON 用数组，避免整数 map key 的反序列化坑）
    #[serde(default)]
    pub bits: Vec<BitDto>,
    /// 写值下限（工程量）；None = 不限制
    #[serde(default)]
    pub min: Option<f64>,
    /// 写值上限（工程量）；None = 不限制
    #[serde(default)]
    pub max: Option<f64>,
    /// 写值步长（工程量）；None = 不限制
    #[serde(default)]
    pub step: Option<f64>,
    /// 枚举写值表（仅整数类型；值按工程量口径）
    #[serde(rename = "enum", default)]
    pub value_enum: Vec<EnumValueDto>,
}

/// 枚举点的一个可取项。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumValueDto {
    pub value: i64,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitDto {
    pub bit: u8,
    pub name: String,
    /// 置位文案（缺省 "1"）
    pub on: Option<String>,
    /// 复位文案（缺省 "0"）
    pub off: Option<String>,
}

// —— 仪表盘（工程布局，存 gatewayd；UI 可重装）——

/// 一个仪表盘：12 列网格 + 若干 widget。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardDto {
    pub id: String,
    pub name: String,
    /// 网格列数（默认 12）
    #[serde(default = "default_dashboard_cols")]
    pub cols: u32,
    /// 作用域：缺省 = 系统全局看板；"device" = 某台设备的工作区看板
    #[serde(default)]
    pub scope_kind: Option<String>,
    #[serde(default)]
    pub scope_id: Option<String>,
    #[serde(default)]
    pub widgets: Vec<WidgetDto>,
}

fn default_dashboard_cols() -> u32 {
    12
}

/// 仪表盘组件。`kind` 是组件类型（内置 value/switch/gauge，未来插件 widget 同构）；
/// `config` 为组件专属参数，宿主不解析（未来第三方 widget 可直接扩展）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetDto {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub title: String,
    /// 绑定的 tag：`<设备id>.<点id>`（位子 tag 形如 `dev.status.b0`）；空串 = 未绑定占位
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub config: serde_json::Value,
    pub layout: WidgetLayout,
}

/// 网格坐标（0 基，单位 = 格）。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetLayout {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

// —— UI 插件（ESM；浏览器侧动态 import gatewayd 的静态资源）——

/// 已安装的 UI 插件。入口文件通过 `GET /plugins/<id>/<entry>` 提供。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPluginDto {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    pub sdk_version: u32,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default = "default_plugin_entry")]
    pub entry: String,
    #[serde(default)]
    pub widgets: Vec<UiWidgetDto>,
    #[serde(default)]
    pub pages: Vec<UiPageDto>,
    #[serde(default)]
    pub device_panels: Vec<UiDevicePanelDto>,
}

fn default_plugin_entry() -> String {
    "index.js".into()
}

/// 仪表盘组件贡献点。前端渲染 kind 约定为 `plugin:<插件id>:<widget id>`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiWidgetDto {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_widget_w")]
    pub default_w: u32,
    #[serde(default = "default_widget_h")]
    pub default_h: u32,
}

fn default_widget_w() -> u32 {
    3
}

fn default_widget_h() -> u32 {
    2
}

/// 导航页贡献点；前端路由为 `/plugin/<插件id>/<page id>`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPageDto {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: String,
}

/// 设备详情面板贡献点（v1 仅展示，不渲染）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiDevicePanelDto {
    pub id: String,
    pub title: String,
}

/// 供 CLI/测试快速构造变更消息。
pub fn changes(items: Vec<TagChange>) -> ServerMsg {
    ServerMsg::Changes { items }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmi_domain::value::Quality;

    #[test]
    fn client_msg_roundtrip() {
        let m = ClientMsg::Subscribe {
            tags: vec!["pm1.voltage_ab".into(), "pm1.status.b0".into()],
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"subscribe""#), "{j}");
        let back: ClientMsg = serde_json::from_str(&j).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn server_msg_roundtrip() {
        let m = ServerMsg::Changes {
            items: vec![TagChange {
                tag: hmi_domain::value::TagId::new("pm1.voltage_ab"),
                value: Value::F64(220.1),
                quality: Quality::Good,
                ts_ms: 1700000000000,
            }],
        };
        let j = serde_json::to_string(&m).unwrap();
        assert!(j.contains(r#""type":"changes""#), "{j}");
        let back: ServerMsg = serde_json::from_str(&j).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn hello_shape() {
        let m = ClientMsg::Hello {
            token: "abc".into(),
        };
        let j = serde_json::to_string(&m).unwrap();
        assert_eq!(j, r#"{"type":"hello","token":"abc"}"#);
    }

    #[test]
    fn dashboard_msg_roundtrip() {
        let d = DashboardDto {
            id: "main".into(),
            name: "总览".into(),
            cols: 12,
            scope_kind: None,
            scope_id: None,
            widgets: vec![WidgetDto {
                id: "w1".into(),
                kind: "gauge".into(),
                title: "AB 线电压".into(),
                tag: "pm1.uab".into(),
                unit: "V".into(),
                config: serde_json::json!({ "min": 180, "max": 260 }),
                layout: WidgetLayout {
                    x: 0,
                    y: 0,
                    w: 3,
                    h: 2,
                },
            }],
        };
        let msg = ClientMsg::SaveDashboard {
            dashboard: d.clone(),
        };
        let j = serde_json::to_string(&msg).unwrap();
        assert!(j.contains(r#""type":"saveDashboard""#), "{j}");
        let back: ClientMsg = serde_json::from_str(&j).unwrap();
        assert_eq!(msg, back);

        let reply = ServerMsg::Dashboards {
            dashboards: vec![d],
        };
        let j = serde_json::to_string(&reply).unwrap();
        assert!(j.contains(r#""type":"dashboards""#), "{j}");
        let back: ServerMsg = serde_json::from_str(&j).unwrap();
        assert_eq!(reply, back);
    }

    /// cols 缺省为 12：老版本 UI/手写文件不必写全字段。
    #[test]
    fn dashboard_cols_defaults_to_12() {
        let j = r#"{"id":"main","name":"总览","widgets":[]}"#;
        let d: DashboardDto = serde_json::from_str(j).unwrap();
        assert_eq!(d.cols, 12);
        assert_eq!(d.scope_kind, None);
        assert_eq!(d.scope_id, None);
        assert!(d.widgets.is_empty());
    }

    #[test]
    fn ui_plugin_msg_roundtrip() {
        let plugin = UiPluginDto {
            id: "com.example.meter-card".into(),
            name: "示例卡片".into(),
            version: "1.0.0".into(),
            description: "demo".into(),
            author: "plan_M".into(),
            sdk_version: 1,
            permissions: vec!["tags:subscribe".into()],
            entry: "index.js".into(),
            widgets: vec![UiWidgetDto {
                id: "meter".into(),
                name: "仪表卡片".into(),
                description: String::new(),
                default_w: 3,
                default_h: 2,
            }],
            pages: vec![UiPageDto {
                id: "about".into(),
                title: "关于".into(),
                icon: "plugin".into(),
            }],
            device_panels: vec![],
        };
        let msg = ServerMsg::UiPlugins {
            plugins: vec![plugin],
        };
        let j = serde_json::to_string(&msg).unwrap();
        assert!(j.contains(r#""type":"uiPlugins""#), "{j}");
        assert!(j.contains(r#""sdkVersion":1"#), "{j}");
        let back: ServerMsg = serde_json::from_str(&j).unwrap();
        assert_eq!(msg, back);

        let req = ClientMsg::ImportUiPlugin {
            path: "/tmp/plugin".into(),
        };
        let j = serde_json::to_string(&req).unwrap();
        assert!(j.contains(r#""type":"importUiPlugin""#), "{j}");
        assert_eq!(serde_json::from_str::<ClientMsg>(&j).unwrap(), req);
    }

    #[test]
    fn package_ui_and_workspace_roundtrip() {
        let page = DevicePageDto {
            id: "overview".into(),
            title: "设备概览".into(),
            icon: "pulse".into(),
            components: vec![DevicePageComponentDto {
                id: None,
                kind: "overview".into(),
                title: String::new(),
                bind: Default::default(),
                rect: None,
                config: serde_json::json!({ "compact": true }),
            }],
        };
        let pkg = PackageUiDto {
            pages: vec![page.clone()],
            widgets: vec![],
        };
        let msg = ClientMsg::SaveWorkspace {
            workspace: WorkspaceDto {
                device_type: "com.acme.pm3000".into(),
                pages: pkg.pages.clone(),
            },
        };
        let j = serde_json::to_string(&msg).unwrap();
        assert!(j.contains(r#""type":"saveWorkspace""#), "{j}");
        assert!(j.contains(r#""deviceType":"com.acme.pm3000""#), "{j}");
        let back: ClientMsg = serde_json::from_str(&j).unwrap();
        assert_eq!(back, msg);

        let reply = ServerMsg::Workspaces {
            workspaces: vec![WorkspaceDto {
                device_type: "com.acme.pm3000".into(),
                pages: vec![page],
            }],
        };
        let j = serde_json::to_string(&reply).unwrap();
        assert!(j.contains(r#""type":"workspaces""#), "{j}");
        assert_eq!(serde_json::from_str::<ServerMsg>(&j).unwrap(), reply);
    }

    /// 回归：点表 DTO 必须能由 UI 端反序列化（bits 若用 u8-key map 会在此失败）。
    #[test]
    fn snapshot_with_bit_points_roundtrip() {
        let snap = SnapshotData {
            channels: vec![ChannelDto {
                config: ChannelConfig {
                    id: "ch1".into(),
                    name: "总线".into(),
                    timing: hmi_domain::config::ChannelTiming::default(),
                    transport: hmi_domain::config::TransportConfig::Serial(
                        hmi_domain::config::SerialParams {
                            port: "COM3".into(),
                            hw_id: None,
                            baud_rate: 9600,
                            data_bits: 8,
                            parity: hmi_domain::config::Parity::None,
                            stop_bits: 1,
                            inter_frame_gap_ms: 5,
                        },
                    ),
                },
                state: "running".into(),
                config_error: None,
            }],
            devices: vec![],
            packages: vec![PackageDto {
                id: "com.acme.pm3000".into(),
                name: "PM3000".into(),
                vendor: "ACME".into(),
                version: "1.2.0".into(),
                protocol: ProtocolDecl {
                    driver: "modbus".into(),
                    transports: vec![TransportKind::Serial, TransportKind::Tcp],
                    params: serde_json::json!({ "byteOrder": "abcd" }),
                },
                description: String::new(),
                ui: PackageUiDto::default(),
                points: vec![PointDto {
                    id: "status".into(),
                    name: "状态字".into(),
                    address: PointAddress::Register("40300".parse().unwrap()),
                    area: Some(RegArea::Holding),
                    data_type: DataType::U16,
                    byte_order: None,
                    access: Access::R,
                    scale: None,
                    offset: None,
                    unit: String::new(),
                    poll_group: None,
                    bits: vec![BitDto {
                        bit: 0,
                        name: "运行".into(),
                        on: None,
                        off: None,
                    }],
                    min: None,
                    max: None,
                    step: None,
                    value_enum: vec![],
                }],
            }],
            tags: vec![],
        };
        let msg = ServerMsg::Snapshot(snap);
        let j = serde_json::to_string(&msg).unwrap();
        assert!(j.contains(r#""bits":[{"bit":0"#), "{j}");
        assert!(j.contains(r#""id":"ch1""#), "{j}");
        assert!(j.contains(r#""transport":"serial""#), "{j}");
        assert!(j.contains(r#""state":"running""#), "{j}");
        let back: ServerMsg = serde_json::from_str(&j).unwrap();
        assert_eq!(msg, back);
    }

    /// 槽位绑定既接受单个声明，也接受数组（多点槽位）。
    #[test]
    fn binding_spec_accepts_one_or_many() {
        let one: BindingSpecDto = serde_json::from_str(r#"{ "point": "uab" }"#).unwrap();
        assert_eq!(one.decls().len(), 1);
        assert_eq!(one.decls()[0].point.as_deref(), Some("uab"));

        let many: BindingSpecDto =
            serde_json::from_str(r#"[{ "point": "uab" }, { "point": "ia" }]"#).unwrap();
        assert_eq!(many.decls().len(), 2);
        assert_eq!(many.decls()[1].point.as_deref(), Some("ia"));

        let j = serde_json::to_string(&many).unwrap();
        assert_eq!(j, r#"[{"point":"uab"},{"point":"ia"}]"#);
    }

    /// P3：设备包 ESM 组件声明随包 DTO 下发（UI 据此注册 kind 与入口 URL）。
    #[test]
    fn package_widget_declarations_roundtrip() {
        let mut bindings = BTreeMap::new();
        bindings.insert(
            "series".to_string(),
            WidgetBindingDto {
                kind: "point".into(),
                many: true,
                write: false,
                required: true,
            },
        );
        let pkg = PackageDto {
            id: "com.acme.pm3000".into(),
            name: "PM3000".into(),
            vendor: "ACME".into(),
            version: "1.2.0".into(),
            protocol: ProtocolDecl {
                driver: "modbus".into(),
                transports: vec![TransportKind::Serial],
                params: serde_json::json!({}),
            },
            description: String::new(),
            ui: PackageUiDto {
                pages: vec![],
                widgets: vec![PackageWidgetDto {
                    id: "trend".into(),
                    title: "趋势图".into(),
                    description: String::new(),
                    entry: "widgets/trend.js".into(),
                    size: WidgetSizeDto::default(),
                    bindings,
                    config: serde_json::json!({ "windowSec": 60 }),
                    permissions: vec!["tags:read".into()],
                }],
            },
            points: vec![],
        };
        let j = serde_json::to_string(&pkg).unwrap();
        assert!(j.contains(r#""entry":"widgets/trend.js""#), "{j}");
        assert!(j.contains(r#""many":true"#), "{j}");
        let back: PackageDto = serde_json::from_str(&j).unwrap();
        assert_eq!(back, pkg);
    }
}
