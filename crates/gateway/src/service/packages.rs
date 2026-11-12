//! 设备包管理：安装/删除/重扫 + 包编辑器草稿（校验/安装/导出）。

use super::GatewayService;
use hmi_domain::points::PointsTable;
use hmi_packaging::{
    install as install_package, InstalledPackage, PackageManifest, PackageRegistry,
    ValidationIssues,
};

impl GatewayService {
    /// 已安装包列表（领域类型；wire 映射在 gatewayd）。
    pub async fn packages(&self) -> Vec<InstalledPackage> {
        let packages = self.packages.read().await.clone();
        packages.list().to_vec()
    }

    /// 安装设备包（目录或 zip），返回安装后的落盘信息。
    pub async fn import_package(&self, path: &str) -> Result<InstalledPackage, String> {
        let installed = install_package(
            std::path::Path::new(path),
            &self.data_dir.join("packages"),
            &self.host_version,
            &self.driver_list,
        )
        .map_err(|e| e.to_string())?;
        self.reload_packages().await;
        Ok(installed)
    }

    /// 删除设备包（该 id 的全部版本目录）；有设备在用时拒绝，未安装报错。
    pub async fn remove_package(&self, id: &str) -> Result<(), String> {
        if !super::valid_id(id) {
            return Err(format!("设备包 id 非法：{id}"));
        }
        let in_use = self
            .config
            .read()
            .await
            .devices
            .iter()
            .filter(|d| d.package == id)
            .count();
        if in_use > 0 {
            return Err(format!(
                "该设备包正被 {in_use} 台设备使用，请先删除这些设备再移除"
            ));
        }
        {
            let registry = self.packages.read().await;
            if registry.get(id).is_none() {
                return Err(format!("设备包 `{id}` 未安装"));
            }
        }
        let dir = self.data_dir.join("packages").join(id);
        std::fs::remove_dir_all(&dir).map_err(|e| format!("删除设备包目录失败：{e}"))?;
        self.reload_packages().await;
        Ok(())
    }

    pub(crate) async fn reload_packages(&self) {
        let registry = PackageRegistry::scan(&self.data_dir.join("packages"), &self.driver_list);
        for (path, why) in &registry.broken {
            tracing::warn!(pkg = %path.display(), reason = %why, "跳过损坏的设备包");
        }
        *self.packages.write().await = Arc::new(registry);
    }

    // —— 包编辑器：草稿校验 / 安装 / 导出 ——

    /// 校验 UI 生成的包草稿（不落盘）。
    pub fn validate_package_draft(
        &self,
        manifest_json: &serde_json::Value,
        points_json: &serde_json::Value,
    ) -> Result<ValidationIssues, String> {
        let (manifest, points) = parse_package_draft(manifest_json, points_json)?;
        Ok(hmi_packaging::validate_package(
            &manifest,
            &points,
            &self.driver_list,
        ))
    }

    /// 安装 UI 生成的包草稿：写到临时目录后走正式安装流（同一套校验/落盘规则）。
    pub async fn install_draft_package(
        &self,
        manifest_json: &serde_json::Value,
        points_json: &serde_json::Value,
    ) -> Result<InstalledPackage, String> {
        let (manifest, points) = parse_package_draft(manifest_json, points_json)?;
        let issues = hmi_packaging::validate_package(&manifest, &points, &self.driver_list);
        if !issues.ok() {
            return Err(format!(
                "包校验未通过：\n  - {}",
                issues.errors.join("\n  - ")
            ));
        }
        let dir = std::env::temp_dir().join(format!(
            "hmi-draft-pkg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败：{e}"))?;
        let written = (|| -> Result<(), String> {
            let m = serde_json::to_string_pretty(manifest_json)
                .map_err(|e| format!("序列化 manifest 失败：{e}"))?;
            let p = serde_json::to_string_pretty(points_json)
                .map_err(|e| format!("序列化点表失败：{e}"))?;
            std::fs::write(dir.join("manifest.json"), m)
                .map_err(|e| format!("写入 manifest 失败：{e}"))?;
            std::fs::write(dir.join("points.json"), p).map_err(|e| format!("写入点表失败：{e}"))?;
            Ok(())
        })();
        let result = written.and_then(|()| {
            install_package(
                &dir,
                &self.data_dir.join("packages"),
                &self.host_version,
                &self.driver_list,
            )
            .map_err(|e| e.to_string())
        });
        let _ = std::fs::remove_dir_all(&dir);
        let installed = result?;
        self.reload_packages().await;
        Ok(installed)
    }

    /// 导出包草稿到本机目录（写 manifest.json + points.json，保留原始 JSON 形态）。
    pub fn export_package_draft(
        &self,
        manifest_json: &serde_json::Value,
        points_json: &serde_json::Value,
        dir: &str,
    ) -> Result<(), String> {
        if dir.trim().is_empty() {
            return Err("请填写导出目录".into());
        }
        let (manifest, points) = parse_package_draft(manifest_json, points_json)?;
        let issues = hmi_packaging::validate_package(&manifest, &points, &self.driver_list);
        if !issues.ok() {
            return Err(format!(
                "包校验未通过：\n  - {}",
                issues.errors.join("\n  - ")
            ));
        }
        let dir = std::path::Path::new(dir);
        if dir.exists() && !dir.is_dir() {
            return Err("导出路径不是目录".into());
        }
        std::fs::create_dir_all(dir).map_err(|e| format!("创建导出目录失败：{e}"))?;
        let m = serde_json::to_string_pretty(manifest_json)
            .map_err(|e| format!("序列化 manifest 失败：{e}"))?;
        let p = serde_json::to_string_pretty(points_json)
            .map_err(|e| format!("序列化点表失败：{e}"))?;
        std::fs::write(dir.join("manifest.json"), m)
            .map_err(|e| format!("写入 manifest 失败：{e}"))?;
        std::fs::write(dir.join("points.json"), p).map_err(|e| format!("写入点表失败：{e}"))?;
        Ok(())
    }
}

/// 解析包草稿并归一化（v2 widgets[] → components；包内组件短名 → 全局 kind）。
fn parse_package_draft(
    manifest_json: &serde_json::Value,
    points_json: &serde_json::Value,
) -> Result<(PackageManifest, PointsTable), String> {
    let mut manifest: PackageManifest = serde_json::from_value(manifest_json.clone())
        .map_err(|e| format!("manifest 解析失败：{e}"))?;
    manifest.ui.normalize(&manifest.id);
    let points: PointsTable =
        serde_json::from_value(points_json.clone()).map_err(|e| format!("点表解析失败：{e}"))?;
    Ok((manifest, points))
}

use std::sync::Arc;
