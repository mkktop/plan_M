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
        let cfg: ProjectConfig = serde_json::from_str(&text)
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

#[cfg(test)]
mod tests {
    use super::*;

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
