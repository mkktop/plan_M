//! 工程配置（config.json）的唯一真相源：通道 + 设备实例。
//!
//! 通道 = 一条传输连接（串口 / TCP / 虚拟总线 / 未来的 BLE），
//! 设备 = 挂在该通道上的一个协议地址。协议语义（从站号、点地址）
//! 由设备包与驱动解释，配置层只保存不透明地址。

use crate::link::TransportKind;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 通道通用时序参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelTiming {
    /// 单事务应答超时（毫秒）
    #[serde(default = "default_timeout_ms")]
    pub resp_timeout_ms: u32,
    /// 事务失败重试次数（重试满标记从站离线）
    #[serde(default = "default_retries")]
    pub retries: u32,
    /// 设备离线后的探测周期（毫秒）
    #[serde(default = "default_probe_interval_ms")]
    pub probe_interval_ms: u64,
}

impl Default for ChannelTiming {
    fn default() -> Self {
        Self {
            resp_timeout_ms: default_timeout_ms(),
            retries: default_retries(),
            probe_interval_ms: default_probe_interval_ms(),
        }
    }
}

fn default_timeout_ms() -> u32 {
    200
}
fn default_retries() -> u32 {
    2
}
fn default_probe_interval_ms() -> u64 {
    5000
}

/// 一条通道。
///
/// 序列化形态（v2）：
/// ```json
/// { "id": "ch1", "name": "现场总线", "transport": "serial",
///   "respTimeoutMs": 200, "retries": 2, "probeIntervalMs": 5000,
///   "port": "COM3", "baudRate": 9600, ... }
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConfig {
    /// 通道实例 id（设备按它挂靠）
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub timing: ChannelTiming,
    #[serde(flatten)]
    pub transport: TransportConfig,
}

impl ChannelConfig {
    pub fn transport_kind(&self) -> TransportKind {
        self.transport.kind()
    }

    /// 事务间静默间隔：串口需要 3.5 字符间隔，TCP/BLE/SIM 为 0。
    pub fn inter_frame_gap(&self) -> Duration {
        Duration::from_millis(self.transport.inter_frame_gap_ms())
    }
}

/// 传输参数联合体。新增传输时在这里加一个变体。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "kebab-case")]
pub enum TransportConfig {
    Serial(SerialParams),
    Tcp(TcpParams),
    Sim(SimParams),
    /// 预留：BLE 透传/原生 GATT。宿主未注册工厂时不可创建。
    Ble(BleParams),
}

impl TransportConfig {
    pub fn kind(&self) -> TransportKind {
        match self {
            TransportConfig::Serial(_) => TransportKind::Serial,
            TransportConfig::Tcp(_) => TransportKind::Tcp,
            TransportConfig::Sim(_) => TransportKind::Sim,
            TransportConfig::Ble(_) => TransportKind::Ble,
        }
    }

    pub fn serial(&self) -> Option<&SerialParams> {
        match self {
            TransportConfig::Serial(p) => Some(p),
            _ => None,
        }
    }

    pub fn inter_frame_gap_ms(&self) -> u64 {
        self.serial().map(|p| p.inter_frame_gap_ms).unwrap_or(0)
    }
}

/// 串口参数（一条 485 总线）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialParams {
    /// 串口名，如 "COM3"
    pub port: String,
    /// 硬件 ID（如 "VID_1A86&PID_7523"），用于 COM 号漂移后重新匹配；可选
    #[serde(default)]
    pub hw_id: Option<String>,
    #[serde(default = "default_baud")]
    pub baud_rate: u32,
    /// 7 或 8
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default)]
    pub parity: Parity,
    /// 1 或 2
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    /// 事务间静默间隔（毫秒），近似 Modbus 的 3.5 字符间隔
    #[serde(default = "default_gap_ms")]
    pub inter_frame_gap_ms: u64,
}

fn default_baud() -> u32 {
    9600
}
fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}
fn default_gap_ms() -> u64 {
    5
}

/// TCP 参数（Modbus TCP 网关 / 串口服务器）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TcpParams {
    pub host: String,
    #[serde(default = "default_tcp_port")]
    pub port: u16,
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u32,
}

fn default_tcp_port() -> u16 {
    502
}
fn default_connect_timeout_ms() -> u32 {
    3000
}

/// 进程内虚拟总线（演示/测试）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimParams {
    /// 演示设备种子名（宿主解释；缺省用通道 id）
    #[serde(default)]
    pub seed: Option<String>,
}

/// BLE 参数（预留）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BleParams {
    /// 适配器 id；None = 系统默认
    #[serde(default)]
    pub adapter: Option<String>,
    /// 外设地址（MAC/UUID）
    pub address: String,
}

/// 校验位。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Parity {
    #[default]
    None,
    Odd,
    Even,
}

impl Parity {
    pub fn as_str(self) -> &'static str {
        match self {
            Parity::None => "none",
            Parity::Odd => "odd",
            Parity::Even => "even",
        }
    }
}

/// 设备地址：驱动私有的寻址值。
///
/// - Modbus：`1`（从站/单元号）
/// - BLE：`"AA:BB:CC:DD:EE:FF"`
/// - MQTT/HTTP：任意稳定标识
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DeviceAddress {
    Number(u32),
    Text(String),
}

impl DeviceAddress {
    pub fn as_number(&self) -> Option<u32> {
        match self {
            DeviceAddress::Number(n) => Some(*n),
            DeviceAddress::Text(_) => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            DeviceAddress::Text(s) => Some(s),
            DeviceAddress::Number(_) => None,
        }
    }
}

impl std::fmt::Display for DeviceAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceAddress::Number(n) => write!(f, "{n}"),
            DeviceAddress::Text(s) => f.write_str(s),
        }
    }
}

impl From<u32> for DeviceAddress {
    fn from(v: u32) -> Self {
        DeviceAddress::Number(v)
    }
}

impl From<String> for DeviceAddress {
    fn from(v: String) -> Self {
        DeviceAddress::Text(v)
    }
}

impl From<&str> for DeviceAddress {
    fn from(v: &str) -> Self {
        DeviceAddress::Text(v.to_string())
    }
}

/// 设备实例：`通道 + 驱动地址 + 设备包 + 覆盖层`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceConfig {
    /// 设备实例 id，同时是 tag 前缀
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// 挂靠的通道 id
    pub channel_id: String,
    /// 设备包 id（如 "com.acme.pm3000"）
    pub package: String,
    /// 驱动私有设备地址
    pub address: DeviceAddress,
    /// 点级覆盖："points.<pointId>.scale" → 0.01、
    /// "points.<pointId>.pollGroup" → "off"、
    /// "points.<pointId>.offset" → 5
    #[serde(default)]
    pub overrides: serde_json::Map<String, serde_json::Value>,
}

/// 整个工程（config.json）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
    #[serde(default)]
    pub devices: Vec<DeviceConfig>,
}

impl ProjectConfig {
    pub fn channel(&self, id: &str) -> Option<&ChannelConfig> {
        self.channels.iter().find(|c| c.id == id)
    }

    pub fn device(&self, id: &str) -> Option<&DeviceConfig> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn devices_on(&self, channel_id: &str) -> Vec<&DeviceConfig> {
        self.devices
            .iter()
            .filter(|d| d.channel_id == channel_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_channel_roundtrip() {
        let j = r#"{
            "id": "ch1", "name": "现场总线", "transport": "serial",
            "port": "COM3", "baudRate": 19200, "parity": "even"
        }"#;
        let c: ChannelConfig = serde_json::from_str(j).unwrap();
        assert_eq!(c.transport_kind(), TransportKind::Serial);
        assert_eq!(c.timing, ChannelTiming::default());
        let p = c.transport.serial().unwrap();
        assert_eq!(p.baud_rate, 19200);
        assert_eq!(p.parity, Parity::Even);
        assert_eq!(p.stop_bits, 1);
        assert_eq!(p.inter_frame_gap_ms, 5);
        assert_eq!(c.inter_frame_gap(), Duration::from_millis(5));

        let back = serde_json::to_string(&c).unwrap();
        assert!(back.contains("\"transport\":\"serial\""));
        assert!(back.contains("\"respTimeoutMs\":200"));
    }

    #[test]
    fn tcp_channel_roundtrip() {
        let j = r#"{
            "id": "ch2", "transport": "tcp", "host": "192.168.1.10", "port": 15020
        }"#;
        let c: ChannelConfig = serde_json::from_str(j).unwrap();
        assert_eq!(c.transport_kind(), TransportKind::Tcp);
        assert_eq!(c.inter_frame_gap(), Duration::ZERO);
        match &c.transport {
            TransportConfig::Tcp(p) => {
                assert_eq!(p.host, "192.168.1.10");
                assert_eq!(p.port, 15020);
                assert_eq!(p.connect_timeout_ms, 3000);
            }
            _ => panic!("应为 tcp"),
        }
    }

    #[test]
    fn device_address_number_or_text() {
        let d: DeviceConfig =
            serde_json::from_str(r#"{ "id":"pm1","channelId":"ch1","package":"a.b","address":3 }"#)
                .unwrap();
        assert_eq!(d.address, DeviceAddress::Number(3));
        assert_eq!(d.address.to_string(), "3");

        let d: DeviceConfig = serde_json::from_str(
            r#"{ "id":"ble1","channelId":"ch2","package":"a.b","address":"AA:BB:CC:DD:EE:FF" }"#,
        )
        .unwrap();
        assert_eq!(d.address, DeviceAddress::Text("AA:BB:CC:DD:EE:FF".into()));
    }

    #[test]
    fn project_config_defaults() {
        let p: ProjectConfig = serde_json::from_str("{}").unwrap();
        assert!(p.channels.is_empty());
        assert!(p.devices.is_empty());
    }
}
