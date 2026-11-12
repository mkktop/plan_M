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
use hmi_domain::config::{DeviceConfig, Parity, SerialChannelConfig};
use hmi_domain::points::Access;
use hmi_domain::value::{TagChange, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// 扫描通道上的从站（范围 1-247，缺省 1-32）
    ScanChannel {
        channel: String,
        range: Option<(u8, u8)>,
    },
    /// 枚举本机串口
    GetSerialPorts,
    /// 已安装设备包清单
    GetPackages,
    /// 新增/更新通道
    UpsertChannel {
        config: SerialChannelConfig,
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
}

// —— 服务端 → 客户端 ——

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ServerMsg {
    /// 握手应答
    Welcome {
        host_version: String,
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
        slave: u8,
        found: bool,
    },
    ScanDone {
        channel: String,
        found: Vec<u8>,
    },
    SerialPorts {
        ports: Vec<SerialPortDto>,
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
    pub id: String,
    pub name: String,
    pub port: String,
    /// running / recovering / faulted / stopped
    pub state: String,
    /// 以下为完整串口参数，供配置页回填（编辑通道不必再猜端口设置）
    pub hw_id: Option<String>,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub parity: Parity,
    pub stop_bits: u8,
    pub resp_timeout_ms: u32,
    pub retries: u32,
    pub inter_frame_gap_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    pub id: String,
    pub name: String,
    pub channel_id: String,
    pub package: String,
    pub slave: u8,
    pub online: bool,
    /// 实例级点表覆盖（scale/offset/pollGroup 等），供设置页回填
    #[serde(default)]
    pub overrides: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDto {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub driver: String,
    #[serde(default)]
    pub description: String,
    /// 设备包声明的默认工作区页面（空 = 宿主 fallback 四页）
    #[serde(default)]
    pub ui: PackageUiDto,
    /// 点位定义，供 UI 做点表展示、写值入口与 tag 名映射
    pub points: Vec<PointDto>,
}

/// 设备包 UI 声明。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageUiDto {
    #[serde(default)]
    pub pages: Vec<DevicePageDto>,
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
    /// 宿主组件：overview / dashboard / tags / settings / core:value / core:gauge
    pub kind: String,
    #[serde(default)]
    pub title: String,
    /// 槽位 → 绑定；包内通常声明相对点（point/bit），用户页面可声明绝对 tag
    #[serde(default)]
    pub bind: BTreeMap<String, BindingDeclDto>,
    /// 12 列网格位置；整页要么全部声明、要么全部省略（流式）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<WidgetRectDto>,
    #[serde(default)]
    pub config: serde_json::Value,
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
    /// 工程记法地址（如 "40101" / "HR:100"）
    pub register: String,
    /// 数据区（位区点位写值走布尔线圈语义）
    pub area: RegArea,
    #[serde(rename = "type")]
    pub data_type: DataType,
    pub access: Access,
    pub scale: Option<f64>,
    pub offset: Option<f64>,
    pub unit: String,
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortDto {
    pub port_name: String,
    pub hw_id: Option<String>,
    pub description: String,
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
                kind: "overview".into(),
                title: String::new(),
                bind: Default::default(),
                rect: None,
                config: serde_json::json!({ "compact": true }),
            }],
        };
        let pkg = PackageUiDto {
            pages: vec![page.clone()],
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
                id: "ch1".into(),
                name: "总线".into(),
                port: "COM3".into(),
                state: "running".into(),
                hw_id: None,
                baud_rate: 9600,
                data_bits: 8,
                parity: Parity::None,
                stop_bits: 1,
                resp_timeout_ms: 200,
                retries: 2,
                inter_frame_gap_ms: 5,
            }],
            devices: vec![],
            packages: vec![PackageDto {
                id: "com.acme.pm3000".into(),
                name: "PM3000".into(),
                vendor: "ACME".into(),
                version: "1.2.0".into(),
                driver: "modbus-rtu".into(),
                description: String::new(),
                ui: PackageUiDto::default(),
                points: vec![PointDto {
                    id: "status".into(),
                    name: "状态字".into(),
                    register: "40300".into(),
                    area: RegArea::Holding,
                    data_type: DataType::U16,
                    access: Access::R,
                    scale: None,
                    offset: None,
                    unit: String::new(),
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
        let back: ServerMsg = serde_json::from_str(&j).unwrap();
        assert_eq!(msg, back);
    }
}
