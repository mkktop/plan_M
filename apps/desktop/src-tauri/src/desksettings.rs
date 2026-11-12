//! 桌面壳自己的设置（独立于 gatewayd 数据目录）：目前只存「数据目录指针」。
//!
//! 指针固定在 `%APPDATA%\planM\desktop.json` —— 它必须固定，
//! 否则「设置存在哪个数据目录」就变成鸡生蛋问题。真正的重数据
//! （通道/设备/仪表盘/设备包/插件）放用户在设置页选择的数据目录。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopConfig {
    /// `None` = 默认 `%APPDATA%\gatewayd`。
    #[serde(default)]
    pub data_dir: Option<String>,
}

/// 设置文件位置（固定，与数据目录无关）。
pub fn config_path() -> PathBuf {
    config_path_at(&base_dir())
}

fn base_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("planM")
}

/// 与 gatewayd 的 `default_data_dir()` 保持一致：`%APPDATA%\gatewayd`。
pub fn default_gateway_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gatewayd")
}

/// 数据目录解析顺序：desktop.json 指针 → 默认 `%APPDATA%\gatewayd`。
pub fn resolve_data_dir() -> PathBuf {
    resolve_data_dir_at(&config_path(), &default_gateway_dir())
}

pub fn load_config() -> Option<DesktopConfig> {
    load_config_at(&config_path())
}

pub fn save_config(cfg: &DesktopConfig) -> Result<(), String> {
    save_config_at(&config_path(), cfg)
}

// —— 纯函数实现（路径可注入，便于单测） ——

pub(crate) fn load_config_at(path: &Path) -> Option<DesktopConfig> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub(crate) fn save_config_at(path: &Path, cfg: &DesktopConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建设置目录失败：{e}"))?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| format!("序列化设置失败：{e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("写入设置失败：{e}"))?;
    // Windows 上 rename 不覆盖已存在文件，先删再改名（与 gateway 侧存储同款做法）
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| format!("更新设置失败：{e}"))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("保存设置失败：{e}"))?;
    Ok(())
}

pub(crate) fn config_path_at(base: &Path) -> PathBuf {
    base.join("desktop.json")
}

/// 数据目录解析（路径可注入，便于单测）。
pub(crate) fn resolve_data_dir_at(cfg_path: &Path, default: &Path) -> PathBuf {
    load_config_at(cfg_path)
        .and_then(|c| c.data_dir)
        .map(PathBuf::from)
        .unwrap_or_else(|| default.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hmi-deskset-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolves_default_when_missing() {
        let base = temp_dir("default");
        let cfg = config_path_at(&base);
        assert!(!cfg.exists());
        let resolved = resolve_data_dir_at(&cfg, Path::new(r"D:\data"));
        assert_eq!(resolved, PathBuf::from(r"D:\data"));
    }

    #[test]
    fn config_roundtrip_and_resolve() {
        let base = temp_dir("roundtrip");
        let cfg = config_path_at(&base);
        save_config_at(
            &cfg,
            &DesktopConfig {
                data_dir: Some(r"D:\planM-data".into()),
            },
        )
        .unwrap();
        // 覆盖写一次，验证先删后改名路径
        save_config_at(
            &cfg,
            &DesktopConfig {
                data_dir: Some(r"E:\x".into()),
            },
        )
        .unwrap();
        let loaded = load_config_at(&cfg).expect("config should load");
        assert_eq!(loaded.data_dir.as_deref(), Some(r"E:\x"));
        assert_eq!(
            resolve_data_dir_at(&cfg, Path::new(r"C:\default")),
            PathBuf::from(r"E:\x")
        );
    }

    #[test]
    fn malformed_config_falls_back_to_default() {
        let base = temp_dir("malformed");
        let cfg = config_path_at(&base);
        std::fs::write(&cfg, "{ not json").unwrap();
        assert_eq!(
            resolve_data_dir_at(&cfg, Path::new(r"C:\default")),
            PathBuf::from(r"C:\default")
        );
    }
}
