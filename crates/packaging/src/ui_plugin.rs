//! UI 插件（ESM）打包：manifest 解析/校验、安装与注册表。
//!
//! 插件目录 = `manifest.json` + 构建好的 ESM 入口（默认 `index.js`）+ 资源。
//! 安装到 `<data-dir>/ui-plugins/<插件id>/`；gatewayd 再通过
//! `GET /plugins/<id>/<entry>` 静态服务给 webview 动态 import。
//!
//! 安全边界 v1：插件与宿主同 webview，权限是**能力门面**（ctx 方法检查 manifest 声明），
//! 不是强沙箱；沙箱化（iframe/worker）留到后续版本。

use crate::install::{
    copy_dir_recursive, locate_pkg_dir, materialize, unique_temp_dir, PackageError,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub const SCHEMA_VERSION: u32 = 1;
/// 宿主实现的插件 SDK 版本；manifest 的 `sdkVersion` 必须 ≤ 它。
pub const SDK_VERSION: u32 = 1;
/// 组件默认尺寸上限（与仪表盘网格列数上限一致）。
pub const MAX_WIDGET_SPAN: u32 = 12;

/// manifest 可声明的权限。未知权限在导入时直接拒绝（挡住拼写错误）。
pub const ALLOWED_PERMISSIONS: &[&str] = &["tags:subscribe", "tags:write", "notify", "theme"];

#[derive(Error, Debug)]
pub enum UiPluginError {
    #[error("{0}")]
    Package(#[from] PackageError),
    #[error("I/O 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("{file} 解析失败（行 {line} 列 {col}）：{msg}")]
    Parse {
        file: String,
        line: usize,
        col: usize,
        msg: String,
    },
    #[error("缺少文件：{0}")]
    MissingFile(String),
    #[error("插件校验未通过：\n  - {0}")]
    Invalid(String),
    #[error("插件 id 非法：{0}")]
    InvalidId(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiPluginManifest {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: Version,
    #[serde(default = "sdk_version")]
    pub sdk_version: u32,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    /// 相对插件目录的 ESM 入口。
    #[serde(default = "default_entry")]
    pub entry: String,
    /// 能力声明（ctx 方法会检查）。
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub widgets: Vec<WidgetContribution>,
    #[serde(default)]
    pub pages: Vec<PageContribution>,
    /// v1 仅解析与展示，暂不渲染设备详情面板。
    #[serde(default)]
    pub device_panels: Vec<DevicePanelContribution>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WidgetContribution {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_widget_w")]
    pub default_w: u32,
    #[serde(default = "default_widget_h")]
    pub default_h: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageContribution {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePanelContribution {
    pub id: String,
    pub title: String,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

fn sdk_version() -> u32 {
    SDK_VERSION
}

fn default_entry() -> String {
    "index.js".into()
}

fn default_widget_w() -> u32 {
    3
}

fn default_widget_h() -> u32 {
    2
}

/// 已安装插件（磁盘态）。
#[derive(Clone, Debug)]
pub struct InstalledUiPlugin {
    pub manifest: UiPluginManifest,
    pub dir: PathBuf,
}

/// 从目录读取并校验插件 manifest。
pub fn load_ui_plugin_dir(dir: &Path) -> Result<UiPluginManifest, UiPluginError> {
    let manifest_path = dir.join("manifest.json");
    if !manifest_path.is_file() {
        return Err(UiPluginError::MissingFile("manifest.json".into()));
    }
    let text = fs::read_to_string(&manifest_path)?;
    let manifest: UiPluginManifest =
        serde_json::from_str(&text).map_err(|e| UiPluginError::Parse {
            file: "manifest.json".into(),
            line: e.line(),
            col: e.column(),
            msg: e.to_string(),
        })?;
    let errors = validate_ui_plugin(&manifest, Some(dir));
    if !errors.is_empty() {
        return Err(UiPluginError::Invalid(errors.join("\n  - ")));
    }
    Ok(manifest)
}

/// 语义校验。`dir` 提供时同时检查 entry 文件是否真实存在。
pub fn validate_ui_plugin(m: &UiPluginManifest, dir: Option<&Path>) -> Vec<String> {
    let mut errors = Vec::new();

    if m.schema_version != SCHEMA_VERSION {
        errors.push(format!(
            "schemaVersion 需为 {SCHEMA_VERSION}，当前 {}",
            m.schema_version
        ));
    }
    if !valid_plugin_id(&m.id) {
        errors.push(format!(
            "插件 id `{}` 非法（字母/数字/点/连字符/下划线，≤128，且须以字母或数字开头）",
            m.id
        ));
    }
    if m.name.trim().is_empty() {
        errors.push("插件 name 不能为空".into());
    } else if m.name.chars().count() > 120 {
        errors.push("插件 name 过长（≤120）".into());
    }
    if m.sdk_version == 0 || m.sdk_version > SDK_VERSION {
        errors.push(format!(
            "sdkVersion 需在 1–{SDK_VERSION}，当前 {}",
            m.sdk_version
        ));
    }

    let mut perms = HashSet::new();
    for p in &m.permissions {
        if !ALLOWED_PERMISSIONS.contains(&p.as_str()) {
            errors.push(format!(
                "未知权限 `{p}`（可用：{}）",
                ALLOWED_PERMISSIONS.join(", ")
            ));
        }
        if !perms.insert(p.as_str()) {
            errors.push(format!("权限 `{p}` 重复"));
        }
    }

    let entry = Path::new(&m.entry);
    if m.entry.trim().is_empty()
        || entry.is_absolute()
        || entry
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        errors.push(format!(
            "entry 必须是插件目录内的相对路径，当前 `{}`",
            m.entry
        ));
    } else {
        let ext = entry
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext != "js" && ext != "mjs" {
            errors.push("entry 必须是 ESM 脚本（.js / .mjs）".into());
        }
        if let Some(dir) = dir {
            if !dir.join(entry).is_file() {
                errors.push(format!("entry 文件不存在：{}", m.entry));
            }
        }
    }

    let mut widget_ids = HashSet::new();
    for w in &m.widgets {
        if !valid_local_id(&w.id) {
            errors.push(format!(
                "widget id `{}` 非法（字母/数字/连字符/下划线，≤64）",
                w.id
            ));
        }
        if !widget_ids.insert(w.id.as_str()) {
            errors.push(format!("widget id `{}` 重复", w.id));
        }
        if w.name.trim().is_empty() {
            errors.push(format!("widget `{}` 缺少 name", w.id));
        }
        if w.default_w == 0 || w.default_w > MAX_WIDGET_SPAN {
            errors.push(format!(
                "widget `{}` 的 defaultW 需在 1–{MAX_WIDGET_SPAN}",
                w.id
            ));
        }
        if w.default_h == 0 || w.default_h > MAX_WIDGET_SPAN {
            errors.push(format!(
                "widget `{}` 的 defaultH 需在 1–{MAX_WIDGET_SPAN}",
                w.id
            ));
        }
    }

    let mut page_ids = HashSet::new();
    for p in &m.pages {
        if !valid_local_id(&p.id) {
            errors.push(format!(
                "page id `{}` 非法（字母/数字/连字符/下划线，≤64）",
                p.id
            ));
        }
        if !page_ids.insert(p.id.as_str()) {
            errors.push(format!("page id `{}` 重复", p.id));
        }
        if p.title.trim().is_empty() {
            errors.push(format!("page `{}` 缺少 title", p.id));
        }
    }

    let mut panel_ids = HashSet::new();
    for p in &m.device_panels {
        if !valid_local_id(&p.id) {
            errors.push(format!(
                "devicePanel id `{}` 非法（字母/数字/连字符/下划线，≤64）",
                p.id
            ));
        }
        if !panel_ids.insert(p.id.as_str()) {
            errors.push(format!("devicePanel id `{}` 重复", p.id));
        }
        if p.title.trim().is_empty() {
            errors.push(format!("devicePanel `{}` 缺少 title", p.id));
        }
    }

    errors
}

/// 安装/升级插件：目录或 zip → 校验 → 落盘 `<root>/<id>/`（同 id 覆盖）。
pub fn install_ui_plugin(src: &Path, dest_root: &Path) -> Result<InstalledUiPlugin, UiPluginError> {
    let work_dir = unique_temp_dir("hmi-ui-plugin-install");
    let result = (|| -> Result<InstalledUiPlugin, UiPluginError> {
        materialize(src, &work_dir)?;
        // zip 常见单层包裹：定位真正含 manifest.json 的目录。
        let plugin_dir = locate_pkg_dir(&work_dir)?;
        let manifest = load_ui_plugin_dir(&plugin_dir)?;

        let target = dest_root.join(&manifest.id);
        if target.exists() {
            fs::remove_dir_all(&target)?;
        }
        fs::create_dir_all(&target)?;
        copy_dir_recursive(&plugin_dir, &target)?;
        Ok(InstalledUiPlugin {
            manifest,
            dir: target,
        })
    })();
    let _ = fs::remove_dir_all(&work_dir);
    result
}

/// 插件注册表：`root/<id>/`，目录为真相源；损坏插件只记录不阻塞服务。
#[derive(Debug, Default)]
pub struct UiPluginRegistry {
    plugins: Vec<InstalledUiPlugin>,
    pub broken: Vec<(PathBuf, String)>,
}

impl UiPluginRegistry {
    pub fn scan(root: &Path) -> Self {
        let mut reg = Self::default();
        let Ok(entries) = fs::read_dir(root) else {
            return reg;
        };
        for e in entries.flatten() {
            let dir = e.path();
            if !dir.is_dir() {
                continue;
            }
            match load_ui_plugin_dir(&dir) {
                Ok(manifest) => reg.plugins.push(InstalledUiPlugin { manifest, dir }),
                Err(err) => reg.broken.push((dir, err.to_string())),
            }
        }
        reg.plugins
            .sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        reg
    }

    pub fn list(&self) -> &[InstalledUiPlugin] {
        &self.plugins
    }

    pub fn get(&self, id: &str) -> Option<&InstalledUiPlugin> {
        self.plugins.iter().find(|p| p.manifest.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

fn valid_plugin_id(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    s.len() <= 128
        && first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn valid_local_id(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    s.len() <= 64
        && first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const MANIFEST: &str = r#"{
        "schemaVersion": 1,
        "id": "com.test.meter-card",
        "name": "测试卡片",
        "version": "1.0.0",
        "sdkVersion": 1,
        "entry": "index.js",
        "permissions": ["tags:subscribe"],
        "widgets": [{ "id": "meter", "name": "卡片", "defaultW": 3, "defaultH": 2 }],
        "pages": [{ "id": "about", "title": "关于" }]
    }"#;

    fn write_plugin(dir: &Path, manifest: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("manifest.json"), manifest).unwrap();
        fs::write(
            dir.join("index.js"),
            "export function mount(el, ctx) { return () => {} }",
        )
        .unwrap();
    }

    #[test]
    fn install_from_dir_and_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let root = tmp.path().join("ui-plugins");
        write_plugin(&src, MANIFEST);

        let installed = install_ui_plugin(&src, &root).unwrap();
        assert_eq!(installed.manifest.id, "com.test.meter-card");
        assert!(installed.dir.join("index.js").is_file());

        let reg = UiPluginRegistry::scan(&root);
        assert_eq!(reg.list().len(), 1);
        assert_eq!(
            reg.get("com.test.meter-card").unwrap().manifest.widgets[0].id,
            "meter"
        );
        assert!(reg.broken.is_empty());
    }

    #[test]
    fn install_from_zip_with_wrapper_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let zip_path = tmp.path().join("plugin.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, content) in [
            ("pkg/manifest.json", MANIFEST),
            ("pkg/index.js", "export function mount() {}"),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();

        let root = tmp.path().join("ui-plugins");
        let installed = install_ui_plugin(&zip_path, &root).unwrap();
        assert!(installed.dir.join("index.js").is_file());
    }

    #[test]
    fn invalid_manifests_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        write_plugin(&src, MANIFEST);

        let mut m: UiPluginManifest = serde_json::from_str(MANIFEST).unwrap();
        m.id = "../evil".into();
        assert!(validate_ui_plugin(&m, Some(&src))
            .iter()
            .any(|e| e.contains("插件 id")));

        let mut m: UiPluginManifest = serde_json::from_str(MANIFEST).unwrap();
        m.permissions = vec!["tags:subscribe".into(), "shell:exec".into()];
        assert!(validate_ui_plugin(&m, Some(&src))
            .iter()
            .any(|e| e.contains("未知权限")));

        let mut m: UiPluginManifest = serde_json::from_str(MANIFEST).unwrap();
        m.sdk_version = 99;
        assert!(validate_ui_plugin(&m, None)
            .iter()
            .any(|e| e.contains("sdkVersion")));

        let mut m: UiPluginManifest = serde_json::from_str(MANIFEST).unwrap();
        m.widgets.push(m.widgets[0].clone());
        assert!(validate_ui_plugin(&m, None)
            .iter()
            .any(|e| e.contains("重复")));

        let mut m: UiPluginManifest = serde_json::from_str(MANIFEST).unwrap();
        m.entry = "../index.js".into();
        assert!(validate_ui_plugin(&m, None)
            .iter()
            .any(|e| e.contains("entry")));

        // entry 缺失（目录检查）
        let m: UiPluginManifest = serde_json::from_str(MANIFEST).unwrap();
        let empty = tmp.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(validate_ui_plugin(&m, Some(&empty))
            .iter()
            .any(|e| e.contains("entry 文件不存在")));
    }

    #[test]
    fn broken_plugin_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ui-plugins");
        let bad = root.join("com.test.bad");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join("manifest.json"), "{ not json").unwrap();

        let reg = UiPluginRegistry::scan(&root);
        assert!(reg.is_empty());
        assert_eq!(reg.broken.len(), 1);
    }

    #[test]
    fn upgrade_replaces_same_id() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ui-plugins");
        let v1 = tmp.path().join("v1");
        let v2 = tmp.path().join("v2");
        write_plugin(&v1, MANIFEST);
        write_plugin(
            &v2,
            &MANIFEST.replace("\"version\": \"1.0.0\"", "\"version\": \"1.2.0\""),
        );

        install_ui_plugin(&v1, &root).unwrap();
        install_ui_plugin(&v2, &root).unwrap();
        let reg = UiPluginRegistry::scan(&root);
        assert_eq!(reg.list().len(), 1);
        assert_eq!(
            reg.get("com.test.meter-card")
                .unwrap()
                .manifest
                .version
                .to_string(),
            "1.2.0"
        );
    }
}
