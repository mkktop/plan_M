//! 工程配置：通道 / 设备实例。config.json 的唯一真相源。

use serde::{Deserialize, Serialize};

/// 串口通道配置（一条 485 总线）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialChannelConfig {
    /// 通道实例 id（tag 不依赖它，但设备按它挂靠）
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// 串口名，如 "COM3"；特殊值 "SIM" 表示虚拟总线（演示/测试）
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
    /// 单事务应答超时（毫秒）
    #[serde(default = "default_timeout_ms")]
    pub resp_timeout_ms: u32,
    /// 事务失败重试次数（重试满标记从站离线）
    #[serde(default = "default_retries")]
    pub retries: u32,
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
fn default_timeout_ms() -> u32 {
    200
}
fn default_retries() -> u32 {
    2
}
fn default_gap_ms() -> u64 {
    5
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

/// 设备实例：`通道 + 从站地址 + 设备包 + 覆盖层`。
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
    /// Modbus 从站地址 1-247
    pub slave: u8,
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
    pub channels: Vec<SerialChannelConfig>,
    #[serde(default)]
    pub devices: Vec<DeviceConfig>,
}

impl ProjectConfig {
    pub fn channel(&self, id: &str) -> Option<&SerialChannelConfig> {
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
    fn config_roundtrip_with_defaults() {
        let j = r#"{
            "id": "ch1", "name": "现场总线", "port": "COM3",
            "baudRate": 19200, "parity": "even"
        }"#;
        let c: SerialChannelConfig = serde_json::from_str(j).unwrap();
        assert_eq!(c.baud_rate, 19200);
        assert_eq!(c.parity, Parity::Even);
        assert_eq!(c.resp_timeout_ms, 200); // 默认
        assert_eq!(c.retries, 2);
        assert_eq!(c.stop_bits, 1);
        assert_eq!(c.inter_frame_gap_ms, 5);

        // 序列化回来含全部字段
        let back = serde_json::to_string(&c).unwrap();
        assert!(back.contains("\"respTimeoutMs\":200"));
    }

    #[test]
    fn project_config_defaults() {
        let p: ProjectConfig = serde_json::from_str("{}").unwrap();
        assert!(p.channels.is_empty());
        assert!(p.devices.is_empty());
    }

    #[test]
    fn device_lookup() {
        let mut p = ProjectConfig::default();
        p.devices.push(DeviceConfig {
            id: "pm1".into(),
            name: String::new(),
            channel_id: "ch1".into(),
            package: "com.acme.pm3000".into(),
            slave: 3,
            overrides: Default::default(),
        });
        assert_eq!(p.devices_on("ch1").len(), 1);
        assert_eq!(p.devices_on("chX").len(), 0);
        assert!(p.device("pm1").is_some());
    }
}
