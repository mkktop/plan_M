//! UI 插件注册表 + 设备包 ESM 组件资源。

use super::GatewayService;
use hmi_packaging::{install_ui_plugin, InstalledUiPlugin, UiPluginRegistry};
use std::path::PathBuf;

impl GatewayService {
    /// 已安装 UI 插件列表（领域类型；wire 映射在 gatewayd）。
    pub async fn ui_plugins(&self) -> Vec<InstalledUiPlugin> {
        self.ui_plugins.read().await.list().to_vec()
    }

    /// 已安装插件的真实目录（目录名不一定等于插件 id：手动拷贝也可能被扫描到）。
    pub async fn ui_plugin_dir(&self, id: &str) -> Option<PathBuf> {
        self.ui_plugins.read().await.get(id).map(|p| p.dir.clone())
    }

    /// 设备包 ESM 组件资源：只暴露清单声明 entry 所在目录下的文件。
    ///
    /// 返回规范化后的真实路径（已校验在包目录内且为普通文件）。
    pub async fn package_widget_asset(
        &self,
        id: &str,
        version: &str,
        relative: &str,
    ) -> Option<PathBuf> {
        let registry = self.packages.read().await.clone();
        let pkg = registry.get(id)?;
        if pkg.manifest.version.to_string() != version {
            return None;
        }
        let rel = std::path::Path::new(relative);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return None;
        }
        let declared = pkg.manifest.ui.widgets.iter().any(|w| {
            let entry = std::path::Path::new(&w.entry);
            let dir = entry.parent().unwrap_or_else(|| std::path::Path::new(""));
            rel == entry || (!dir.as_os_str().is_empty() && rel.starts_with(dir))
        });
        if !declared {
            return None;
        }
        let base = std::fs::canonicalize(&pkg.dir).ok()?;
        let file = std::fs::canonicalize(pkg.dir.join(rel)).ok()?;
        if !file.starts_with(&base) || !file.is_file() {
            return None;
        }
        Some(file)
    }

    /// 安装/升级插件。
    pub async fn import_ui_plugin(&self, path: &str) -> Result<(), String> {
        install_ui_plugin(
            std::path::Path::new(path),
            &self.data_dir.join("ui-plugins"),
        )
        .map_err(|e| e.to_string())?;
        self.reload_ui_plugins().await;
        Ok(())
    }

    /// 删除插件目录；不存在视为成功（幂等）。
    pub async fn remove_ui_plugin(&self, id: &str) -> Result<(), String> {
        if !super::valid_id(id) {
            return Err(format!("插件 id 非法：{id}"));
        }
        let dir = {
            let registry = self.ui_plugins.read().await;
            registry
                .get(id)
                .map(|p| p.dir.clone())
                .unwrap_or_else(|| self.data_dir.join("ui-plugins").join(id))
        };
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("删除插件目录失败：{e}"))?;
        }
        self.reload_ui_plugins().await;
        Ok(())
    }

    async fn reload_ui_plugins(&self) {
        let registry = UiPluginRegistry::scan(&self.data_dir.join("ui-plugins"));
        for (path, why) in &registry.broken {
            tracing::warn!(plugin = %path.display(), reason = %why, "跳过损坏的 UI 插件");
        }
        *self.ui_plugins.write().await = registry;
    }
}
