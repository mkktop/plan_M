//! gatewayd ↔ UI 的 WebSocket 消息契约。
//!
//! 传输形态：文本帧，内容为 `serde_json` 序列化的 [`ClientMsg`] / [`ServerMsg`]，
//! 判别字段 `type`（camelCase）。
//!
//! 握手：连接后客户端第一条消息必须是 [`ClientMsg::Hello`]（携带 token），
//! 服务端回 [`ServerMsg::Welcome`]；token 错误服务端直接断开。
//!
//! 订阅制：UI 只订阅当前画面用到的 tag；`Changes` 为 10~20ms 合并后的增量批量。

use hmi_domain::config::{DeviceConfig, SerialChannelConfig};
use hmi_domain::value::{TagChange, Value};
use serde::{Deserialize, Serialize};

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
    /// 导入设备包（gatewayd 本机路径）
    ImportPackage {
        path: String,
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDto {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub driver: String,
    /// 点位概要（id/name/unit），供 UI 做点表展示与 tag 名映射
    pub points: Vec<PointDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointDto {
    pub id: String,
    pub name: String,
    pub unit: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortDto {
    pub port_name: String,
    pub hw_id: Option<String>,
    pub description: String,
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
}
