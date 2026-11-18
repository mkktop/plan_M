//! 设备包管理：安装/删除/重扫 + 包编辑器草稿（校验/安装/导出）。

use super::GatewayService;
use hmi_domain::{config::ProjectConfig, points::PointsTable};
use hmi_packaging::{
    install_checked, load_package_dir, InstalledPackage, PackageError, PackageManifest,
    ValidationIssues, ACTIVE_VERSION_FILE,
};
use semver::Version;
use std::collections::BTreeSet;
use std::sync::Arc;

impl GatewayService {
    /// 已安装包列表（领域类型；wire 映射在 gatewayd）。
    pub async fn packages(&self) -> Vec<InstalledPackage> {
        let _activation = self.activation.read().await;
        let packages = self.packages.read().await.clone();
        packages.list().to_vec()
    }

    /// 安装设备包（目录或 zip），返回安装后的落盘信息。
    pub async fn import_package(self: &Arc<Self>, path: &str) -> Result<InstalledPackage, String> {
        let _activation = self.activation.write().await;
        let config = self.config.read().await.clone();
        let active = self.packages.read().await.clone();
        let mut affected = BTreeSet::new();
        let mut activate = false;
        let installed = install_checked(
            std::path::Path::new(path),
            &self.data_dir.join("packages"),
            &self.host_version,
            &self.driver_list,
            |candidate| {
                let id = &candidate.manifest.id;
                activate = active
                    .get(id)
                    .is_none_or(|current| candidate.manifest.version >= current.manifest.version);
                if !activate {
                    return Ok(());
                }
                affected = self.channels_for_package(&config, candidate)?;
                Ok(())
            },
        )
        .map_err(|error| error.to_string())?;
        if activate {
            let pinned = installed
                .dir
                .parent()
                .expect("package version has a parent")
                .join(ACTIVE_VERSION_FILE);
            if pinned.exists()
                && active
                    .get(&installed.manifest.id)
                    .is_some_and(|previous| previous.manifest.version != installed.manifest.version)
            {
                self.pin_package(&installed)?;
            }
            self.apply_package(installed.clone(), affected).await;
        }
        Ok(installed)
    }

    fn channels_for_package(
        &self,
        config: &ProjectConfig,
        package: &InstalledPackage,
    ) -> Result<BTreeSet<String>, PackageError> {
        let mut affected = BTreeSet::new();
        for device in config
            .devices
            .iter()
            .filter(|device| device.package == package.manifest.id)
        {
            let channel = config.channel(&device.channel_id).ok_or_else(|| {
                PackageError::Invalid(format!("设备 {} 所在通道不存在", device.id))
            })?;
            self.prepare_device(device, channel.transport_kind(), package)
                .map_err(|e| PackageError::Invalid(format!("设备 {}：{e}", device.id)))?;
            affected.insert(channel.id.clone());
        }
        Ok(affected)
    }

    async fn apply_package(
        self: &Arc<Self>,
        package: InstalledPackage,
        channels: BTreeSet<String>,
    ) {
        Arc::make_mut(&mut *self.packages.write().await).activate(package);
        for id in channels {
            self.restart_channel(&id).await;
        }
    }

    fn pin_package(&self, package: &InstalledPackage) -> Result<(), String> {
        let root = package.dir.parent().ok_or("设备包版本目录无效")?;
        let target = root.join(ACTIVE_VERSION_FILE);
        let pending = root.join(".active-version.tmp");
        std::fs::write(&pending, package.manifest.version.to_string())
            .and_then(|_| std::fs::rename(&pending, &target))
            .map_err(|e| format!("保存启用版本失败：{e}"))
    }

    pub async fn package_versions(&self, id: &str) -> Result<Vec<Version>, String> {
        let _activation = self.activation.read().await;
        if !super::valid_id(id) || self.packages.read().await.get(id).is_none() {
            return Err("设备包未安装".into());
        }
        let root = self.data_dir.join("packages").join(id);
        let mut versions: Vec<_> = std::fs::read_dir(root)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| Version::parse(entry.file_name().to_str()?).ok())
            .collect();
        versions.sort_by(|a, b| b.cmp(a));
        Ok(versions)
    }

    pub async fn activate_package_version(
        self: &Arc<Self>,
        id: &str,
        version: &Version,
    ) -> Result<(), String> {
        let _activation = self.activation.write().await;
        if !super::valid_id(id) {
            return Err("设备包 id 无效".into());
        }
        let dir = self
            .data_dir
            .join("packages")
            .join(id)
            .join(version.to_string());
        let loaded = load_package_dir(&dir, &self.driver_list).map_err(|e| e.to_string())?;
        if loaded.manifest.id != id || &loaded.manifest.version != version {
            return Err("设备包清单和版本目录不一致".into());
        }
        if loaded
            .manifest
            .min_host_version
            .as_ref()
            .is_some_and(|need| need > &self.host_version)
        {
            return Err("此版本需要更新的网关".into());
        }
        let package = InstalledPackage {
            manifest: loaded.manifest,
            points: loaded.points,
            dir,
        };
        let affected = self
            .channels_for_package(&*self.config.read().await, &package)
            .map_err(|e| e.to_string())?;
        self.pin_package(&package)?;
        self.apply_package(package, affected).await;
        Ok(())
    }

    /// 删除设备包（该 id 的全部版本目录）；有设备在用时拒绝，未安装报错。
    pub async fn remove_package(&self, id: &str) -> Result<(), String> {
        let _activation = self.activation.write().await;
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
        Arc::make_mut(&mut *self.packages.write().await).remove(id);
        Ok(())
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
        self: &Arc<Self>,
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
        let result = match written {
            Ok(()) => self.import_package(&dir.to_string_lossy()).await,
            Err(error) => Err(error),
        };
        let _ = std::fs::remove_dir_all(&dir);
        result
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
