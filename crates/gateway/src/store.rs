//! 工程配置存储：v1 用 JSON 文件（原子写），trait 留 SQLite 后路。

use hmi_domain::config::ProjectConfig;
use std::path::{Path, PathBuf};

pub trait ConfigStore: Send + Sync {
    fn load(&self) -> anyhow::Result<ProjectConfig>;
    fn save(&self, cfg: &ProjectConfig) -> anyhow::Result<()>;
}

/// JSON 文件实现：`config.json`，临时文件 + rename 原子替换。
pub struct JsonConfigStore {
    path: PathBuf,
}

impl JsonConfigStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ConfigStore for JsonConfigStore {
    fn load(&self) -> anyhow::Result<ProjectConfig> {
        if !self.path.exists() {
            return Ok(ProjectConfig::default());
        }
        let text = std::fs::read_to_string(&self.path)?;
        let cfg: ProjectConfig = decode_project(&text)
            .map_err(|e| anyhow::anyhow!("解析 {} 失败: {e}", self.path.display()))?;
        Ok(cfg)
    }

    fn save(&self, cfg: &ProjectConfig) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(cfg)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// Migrate the serial-only project shape in memory. The next successful edit persists v2.
fn decode_project(text: &str) -> serde_json::Result<ProjectConfig> {
    let mut raw: serde_json::Value = serde_json::from_str(text)?;
    if let Some(channels) = raw.get_mut("channels").and_then(|v| v.as_array_mut()) {
        for channel in channels.iter_mut().filter_map(|v| v.as_object_mut()) {
            if channel.contains_key("transport") || !channel.contains_key("port") {
                continue;
            }
            let transport = if channel.get("port").and_then(|v| v.as_str()) == Some("SIM") {
                "sim"
            } else {
                "serial"
            };
            channel.insert("transport".into(), transport.into());
            if !channel.contains_key("timing") {
                let timing = ["respTimeoutMs", "retries", "probeIntervalMs"]
                    .into_iter()
                    .filter_map(|key| channel.remove(key).map(|value| (key.to_string(), value)))
                    .collect::<serde_json::Map<_, _>>();
                channel.insert("timing".into(), timing.into());
            }
        }
    }
    if let Some(devices) = raw.get_mut("devices").and_then(|v| v.as_array_mut()) {
        for device in devices.iter_mut().filter_map(|v| v.as_object_mut()) {
            if !device.contains_key("address") {
                if let Some(slave) = device.remove("slave") {
                    device.insert("address".into(), slave);
                }
            }
        }
    }
    serde_json::from_value(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_serial_and_sim_channels_keep_device_addresses_and_timing() {
        let config = decode_project(
            r#"{
            "channels":[{"id":"field","port":"COM7","respTimeoutMs":325,"retries":3},
                        {"id":"demo","port":"SIM"}],
            "devices":[{"id":"meter","channelId":"field","package":"com.test.meter","slave":17}]
        }"#,
        )
        .unwrap();
        assert_eq!(config.channels[0].transport.serial().unwrap().port, "COM7");
        assert_eq!(config.channels[0].timing.resp_timeout_ms, 325);
        assert_eq!(config.channels[0].timing.retries, 3);
        assert_eq!(
            config.channels[1].transport_kind(),
            hmi_domain::link::TransportKind::Sim
        );
        assert_eq!(config.devices[0].address.as_number(), Some(17));
    }

    #[test]
    fn roundtrip_and_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = JsonConfigStore::new(dir.path().join("config.json"));

        // 不存在 → 默认空配置
        assert_eq!(store.load().unwrap(), ProjectConfig::default());

        let mut cfg = ProjectConfig::default();
        cfg.channels.push(hmi_domain::config::ChannelConfig {
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
        });
        store.save(&cfg).unwrap();

        let back = store.load().unwrap();
        assert_eq!(back, cfg);
        assert!(!dir.path().join("config.json.tmp").exists()); // 临时文件已换名
    }
}
