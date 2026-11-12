//! 数据目录的查看与迁移（设置页「数据目录」入口）。
//!
//! 迁移只在 gatewayd 由本应用托管时允许：停服务 → 复制数据（不含 logs/）→
//! 把新目录写进 desktop.json 指针 → 用新目录重新拉起。**原目录原样保留作备份**，
//! 绝不删除用户数据。

use crate::desksettings::{self, DesktopConfig};
use crate::supervisor::{SupervisorState, DEFAULT_PORT};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    /// 当前生效的数据目录。
    pub data_dir: String,
    /// 是否仍是默认位置（未迁移过）。
    pub is_default: bool,
    pub default_data_dir: String,
    /// 指针文件位置（固定）。
    pub config_path: String,
    /// gatewayd 是否本应用拉起（迁移需要可安全重启）。
    pub gatewayd_managed: bool,
}

fn storage_info(state: &SupervisorState) -> StorageInfo {
    let current = desksettings::resolve_data_dir();
    let default = desksettings::default_gateway_dir();
    StorageInfo {
        data_dir: current.display().to_string(),
        is_default: current == default,
        default_data_dir: default.display().to_string(),
        config_path: desksettings::config_path().display().to_string(),
        gatewayd_managed: state.is_managed(),
    }
}

#[tauri::command]
pub fn get_storage_info(state: State<'_, SupervisorState>) -> StorageInfo {
    storage_info(&state)
}

/// 把数据目录迁移到 `new_dir` 并用新目录重启采集服务；原目录保留作备份。
#[tauri::command]
pub async fn migrate_data_dir(
    new_dir: String,
    state: State<'_, SupervisorState>,
) -> Result<StorageInfo, String> {
    let new_dir = PathBuf::from(new_dir.trim());
    if new_dir.as_os_str().is_empty() {
        return Err("目录不能为空".into());
    }
    if !state.is_managed() {
        return Err(
            "当前 gatewayd 不是本应用拉起的（外部实例或未运行）。请先关闭外部 gatewayd，由应用重新打开后再迁移"
                .into(),
        );
    }
    let src = desksettings::resolve_data_dir();
    std::fs::create_dir_all(&new_dir).map_err(|e| format!("无法创建目标目录：{e}"))?;
    let src_abs = std::fs::canonicalize(&src).map_err(|e| format!("读取当前目录失败：{e}"))?;
    let dst_abs = std::fs::canonicalize(&new_dir).map_err(|e| format!("读取目标目录失败：{e}"))?;
    if src_abs == dst_abs {
        return Err("目标目录与当前数据目录相同".into());
    }
    if src_abs.starts_with(&dst_abs) || dst_abs.starts_with(&src_abs) {
        return Err("目标目录不能与当前数据目录互相嵌套".into());
    }
    if dir_non_empty(&dst_abs) {
        return Err("目标目录不为空。为避免覆盖已有数据，请选择一个空目录（或新建一个）".into());
    }
    if !src_abs.join("config.json").exists() {
        return Err("当前数据目录缺少 config.json，不是有效的数据目录".into());
    }

    // 停服务 → 复制 → 写指针 → 新目录拉起。任何一步失败都给出明确指引。
    state.shutdown();
    let copied = copy_dir_recursive(&src_abs, &dst_abs, "logs")
        .map_err(|e| format!("复制数据失败：{e}（服务已停止，重开应用即可恢复）"))?;
    desksettings::save_config(&DesktopConfig {
        data_dir: Some(dst_abs.display().to_string()),
    })
    .map_err(|e| format!("保存设置失败：{e}（数据已复制到新目录，旧目录仍有效）"))?;
    state
        .restart(DEFAULT_PORT, dst_abs.clone())
        .map_err(|e| format!("以新目录启动 gatewayd 失败：{e}"))?;
    eprintln!(
        "[storage] 数据目录已迁移：{} → {}（{} 项，原目录保留）",
        src_abs.display(),
        dst_abs.display(),
        copied
    );
    Ok(storage_info(&state))
}

/// 目录里是否已有条目（不含半途创建的空壳）。
pub(crate) fn dir_non_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut it| it.next().is_some())
}

/// 递归复制；顶层名为 `skip` 的条目（logs/）跳过，返回复制的文件数。
pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path, skip: &str) -> Result<u64, String> {
    let mut copied = 0u64;
    let entries = std::fs::read_dir(src).map_err(|e| format!("读取 {src:?}：{e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("枚举 {src:?}：{e}"))?;
        let name = entry.file_name();
        if name.to_string_lossy() == skip {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            std::fs::create_dir_all(&to).map_err(|e| format!("创建 {to:?}：{e}"))?;
            copied += copy_dir_recursive(&from, &to, "")?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| format!("复制 {from:?}：{e}"))?;
            copied += 1;
        }
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hmi-storage-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn copies_tree_but_skips_logs() {
        let src = temp_dir("src");
        std::fs::write(src.join("config.json"), "{}").unwrap();
        std::fs::write(src.join("dashboards.json"), "[]").unwrap();
        std::fs::create_dir_all(src.join("packages").join("com.acme.pm3000")).unwrap();
        std::fs::write(
            src.join("packages")
                .join("com.acme.pm3000")
                .join("manifest.json"),
            "{}x",
        )
        .unwrap();
        std::fs::create_dir_all(src.join("logs")).unwrap();
        std::fs::write(src.join("logs").join("run.log"), "log").unwrap();

        let dst = temp_dir("dst");
        let copied = copy_dir_recursive(&src, &dst, "logs").unwrap();
        assert_eq!(copied, 3); // config + dashboards + manifest
        assert!(dst.join("config.json").exists());
        assert!(dst.join("packages/com.acme.pm3000/manifest.json").exists());
        assert!(!dst.join("logs").exists(), "logs/ 不应被复制");

        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&dst);
    }

    #[test]
    fn dir_non_empty_detects_entries() {
        let dir = temp_dir("nonempty");
        assert!(!dir_non_empty(&dir));
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        assert!(dir_non_empty(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
