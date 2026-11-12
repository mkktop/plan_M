//! 设备类型工作区：`<data-dir>/workspaces.json`。
//!
//! 设备包声明默认页面；这里保存用户对某个类型的覆盖（页面增删/排序/组件组合）。
//! 没有记录时 UI 使用设备包默认页面。布局属工程数据，UI 重装/换机不丢。

use hmi_packaging::ALLOWED_DEVICE_PAGE_COMPONENTS;
use hmi_wire::WorkspaceDto;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: u32 = 1;
const MAX_PAGES: usize = 50;
const MAX_COMPONENTS: usize = 20;
const MAX_TITLE: usize = 120;
const MAX_ICON: usize = 64;

#[derive(Debug, Serialize, Deserialize)]
struct WorkspaceFile {
    #[serde(default = "schema_version")]
    version: u32,
    #[serde(default)]
    workspaces: Vec<WorkspaceDto>,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

/// 工作区存储。损坏文件不阻塞服务（告警后按空处理）。
pub struct WorkspaceStore {
    path: PathBuf,
    workspaces: Vec<WorkspaceDto>,
}

impl WorkspaceStore {
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let workspaces = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<WorkspaceFile>(&text) {
                Ok(file) => file.workspaces,
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "工作区文件解析失败，按无覆盖启动（原文件不会被覆盖，直到下次保存）"
                    );
                    Vec::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "读取工作区文件失败");
                Vec::new()
            }
        };
        Self { path, workspaces }
    }

    pub fn list(&self) -> Vec<WorkspaceDto> {
        self.workspaces.clone()
    }

    pub fn get(&self, device_type: &str) -> Option<&WorkspaceDto> {
        self.workspaces
            .iter()
            .find(|w| w.device_type == device_type)
    }

    /// 新建或覆盖同类型工作区，成功后落盘。
    pub fn upsert(&mut self, workspace: WorkspaceDto) -> Result<(), String> {
        validate_workspace(&workspace)?;
        match self
            .workspaces
            .iter_mut()
            .find(|w| w.device_type == workspace.device_type)
        {
            Some(slot) => *slot = workspace,
            None => self.workspaces.push(workspace),
        }
        self.persist()
    }

    /// 删除类型覆盖（回落到包默认页面）；不存在视为成功。
    pub fn remove(&mut self, device_type: &str) -> Result<(), String> {
        let before = self.workspaces.len();
        self.workspaces.retain(|w| w.device_type != device_type);
        if self.workspaces.len() == before {
            return Ok(());
        }
        self.persist()
    }

    fn persist(&self) -> Result<(), String> {
        let file = WorkspaceFile {
            version: SCHEMA_VERSION,
            workspaces: self.workspaces.clone(),
        };
        let text =
            serde_json::to_string_pretty(&file).map_err(|e| format!("序列化工作区失败：{e}"))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("写入临时文件失败：{e}"))?;
        // Windows 上 rename 不能覆盖已存在文件；先删旧文件。
        #[cfg(windows)]
        {
            if self.path.exists() {
                let _ = std::fs::remove_file(&self.path);
            }
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("替换文件失败：{e}"))?;
        Ok(())
    }
}

/// 校验用户保存的工作区。页面组件 kind 必须是宿主认识的组件。
pub fn validate_workspace(w: &WorkspaceDto) -> Result<(), String> {
    if !valid_device_type(&w.device_type) {
        return Err(format!(
            "设备类型 `{}` 非法（应为小写点分包 id，如 com.acme.pm3000）",
            w.device_type
        ));
    }
    if w.pages.is_empty() {
        return Err("工作区至少需要一个页面".into());
    }
    if w.pages.len() > MAX_PAGES {
        return Err(format!("页面最多 {MAX_PAGES} 个"));
    }

    let mut page_ids = HashSet::new();
    for page in &w.pages {
        if !valid_page_id(&page.id) {
            return Err(format!(
                "页面 id `{}` 非法（字母/数字/下划线/连字符，≤64）",
                page.id
            ));
        }
        if !page_ids.insert(page.id.as_str()) {
            return Err(format!("页面 id `{}` 重复", page.id));
        }
        if page.title.trim().is_empty() {
            return Err(format!("页面 `{}` 缺少标题", page.id));
        }
        if page.title.chars().count() > MAX_TITLE {
            return Err(format!("页面 `{}` 标题过长（≤{MAX_TITLE}）", page.id));
        }
        if page.icon.chars().count() > MAX_ICON {
            return Err(format!("页面 `{}` 图标名过长（≤{MAX_ICON}）", page.id));
        }
        if page.components.is_empty() {
            return Err(format!("页面 `{}` 至少需要一个组件", page.id));
        }
        if page.components.len() > MAX_COMPONENTS {
            return Err(format!("页面 `{}` 组件最多 {MAX_COMPONENTS} 个", page.id));
        }
        let rects: Vec<Option<(u32, u32, u32, u32)>> = page
            .components
            .iter()
            .map(|c| c.rect.map(|r| (r.x, r.y, r.w, r.h)))
            .collect();
        for e in hmi_packaging::validate_component_rects(&page.id, &rects) {
            return Err(e);
        }
        for comp in &page.components {
            if !ALLOWED_DEVICE_PAGE_COMPONENTS.contains(&comp.kind.as_str()) {
                return Err(format!(
                    "页面 `{}` 使用了未知组件 `{}`（可用：{}）",
                    page.id,
                    comp.kind,
                    ALLOWED_DEVICE_PAGE_COMPONENTS.join(", ")
                ));
            }
            if comp.title.chars().count() > MAX_TITLE {
                return Err(format!("页面 `{}` 的组件标题过长（≤{MAX_TITLE}）", page.id));
            }
            if !comp.config.is_null() && !comp.config.is_object() {
                return Err(format!(
                    "页面 `{}` 的组件 `{}` config 必须是 JSON 对象",
                    page.id, comp.kind
                ));
            }
            for (slot, b) in &comp.bind {
                if slot.trim().is_empty() {
                    return Err(format!(
                        "页面 `{}` 的组件 `{}` 存在空绑定槽位",
                        page.id, comp.kind
                    ));
                }
                let has_point = b.point.as_deref().is_some_and(|s| !s.trim().is_empty());
                let has_tag = b.tag.as_deref().is_some_and(|s| !s.trim().is_empty());
                if has_point == has_tag {
                    return Err(format!(
                        "页面 `{}` 的组件 `{}` 槽位 `{slot}`：必须且只能声明 point 或 tag 之一",
                        page.id, comp.kind
                    ));
                }
                if b.bit.is_some() && !has_point {
                    return Err(format!(
                        "页面 `{}` 的组件 `{}` 槽位 `{slot}`：bit 只能与 point 组合",
                        page.id, comp.kind
                    ));
                }
                if has_tag && !valid_tag(b.tag.as_deref().unwrap_or_default()) {
                    return Err(format!(
                        "页面 `{}` 的组件 `{}` 槽位 `{slot}`：tag `{}` 非法",
                        page.id,
                        comp.kind,
                        b.tag.as_deref().unwrap_or_default()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn valid_device_type(s: &str) -> bool {
    let lowered = s.to_lowercase();
    !s.is_empty()
        && s.len() <= 128
        && *s == lowered
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        && s.split('.').count() >= 2
        && s.split('.').all(|part| !part.is_empty())
}

fn valid_page_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 绝对 tag：`<设备id>.<点id>[.b<N>]` 或未来的 `$` 系统命名空间。
fn valid_tag(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 192
        && s.split('.').count() >= 2
        && s.split('.').all(|part| !part.is_empty())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '$'))
}

/// 默认存储路径。
pub fn workspaces_path(data_dir: &Path) -> PathBuf {
    data_dir.join("workspaces.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmi_wire::{DevicePageComponentDto, DevicePageDto};

    fn page(id: &str, kind: &str) -> DevicePageDto {
        DevicePageDto {
            id: id.into(),
            title: id.into(),
            icon: "grid".into(),
            components: vec![DevicePageComponentDto {
                kind: kind.into(),
                title: String::new(),
                bind: Default::default(),
                rect: None,
                config: serde_json::json!({}),
            }],
        }
    }

    fn sample() -> WorkspaceDto {
        WorkspaceDto {
            device_type: "com.acme.pm3000".into(),
            pages: vec![page("overview", "overview"), page("dashboard", "dashboard")],
        }
    }

    #[test]
    fn save_and_reload_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = workspaces_path(tmp.path());
        let mut store = WorkspaceStore::load(&path);
        assert!(store.list().is_empty());

        store.upsert(sample()).unwrap();
        assert_eq!(store.list().len(), 1);
        assert_eq!(WorkspaceStore::load(&path).list(), vec![sample()]);

        // 同类型覆盖，不产生第二个条目
        let mut changed = sample();
        changed.pages.pop();
        store.upsert(changed.clone()).unwrap();
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.get("com.acme.pm3000").unwrap(), &changed);
    }

    #[test]
    fn invalid_workspaces_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = WorkspaceStore::load(workspaces_path(tmp.path()));

        let mut w = sample();
        w.pages.clear();
        assert!(store.upsert(w).is_err());

        let mut w = sample();
        w.pages.push(page("overview", "tags"));
        assert!(store.upsert(w).unwrap_err().contains("重复"));

        let mut w = sample();
        w.pages[0].components[0].kind = "shell".into();
        assert!(store.upsert(w).unwrap_err().contains("未知组件"));

        let mut w = sample();
        w.device_type = "../evil".into();
        assert!(store.upsert(w).is_err());

        let mut w = sample();
        w.pages[0].components.clear();
        assert!(store.upsert(w).unwrap_err().contains("至少需要一个组件"));
    }

    #[test]
    fn malformed_file_is_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let path = workspaces_path(tmp.path());
        std::fs::write(&path, "{ not json").unwrap();
        assert!(WorkspaceStore::load(&path).list().is_empty());
    }

    #[test]
    fn binding_declarations_validated() {
        use hmi_wire::BindingDeclDto;

        let tmp = tempfile::tempdir().unwrap();
        let mut store = WorkspaceStore::load(workspaces_path(tmp.path()));

        let with_bind = |bind: BindingDeclDto| {
            let mut w = sample();
            w.pages[0].components[0] = DevicePageComponentDto {
                kind: "core:value".into(),
                title: String::new(),
                bind: [("value".to_string(), bind)].into_iter().collect(),
                rect: None,
                config: serde_json::json!({}),
            };
            w
        };

        // 用户页面允许相对点与绝对 tag 两种绑定
        assert!(store
            .upsert(with_bind(BindingDeclDto {
                point: Some("uab".into()),
                bit: None,
                tag: None,
            }))
            .is_ok());
        assert!(store
            .upsert(with_bind(BindingDeclDto {
                point: None,
                bit: None,
                tag: Some("pm1.uab".into()),
            }))
            .is_ok());

        let err = store
            .upsert(with_bind(BindingDeclDto {
                point: None,
                bit: None,
                tag: Some("pm1".into()),
            }))
            .unwrap_err();
        assert!(err.contains("非法"), "{err}");

        let err = store
            .upsert(with_bind(BindingDeclDto {
                point: Some("uab".into()),
                bit: None,
                tag: Some("pm1.uab".into()),
            }))
            .unwrap_err();
        assert!(err.contains("只能声明"), "{err}");

        let err = store
            .upsert(with_bind(BindingDeclDto {
                point: None,
                bit: Some(2),
                tag: Some("pm1.status.b2".into()),
            }))
            .unwrap_err();
        assert!(err.contains("bit 只能与 point"), "{err}");
    }

    #[test]
    fn component_rects_validated() {
        use hmi_wire::WidgetRectDto;

        let tmp = tempfile::tempdir().unwrap();
        let mut store = WorkspaceStore::load(workspaces_path(tmp.path()));

        let comp = |kind: &str, rect: Option<WidgetRectDto>| DevicePageComponentDto {
            kind: kind.into(),
            title: String::new(),
            bind: Default::default(),
            rect,
            config: serde_json::json!({}),
        };

        let mut w = sample();
        w.pages[0].components = vec![
            comp(
                "core:value",
                Some(WidgetRectDto {
                    x: 0,
                    y: 0,
                    w: 6,
                    h: 2,
                }),
            ),
            comp(
                "core:gauge",
                Some(WidgetRectDto {
                    x: 3,
                    y: 0,
                    w: 6,
                    h: 2,
                }),
            ),
        ];
        assert!(store.upsert(w).unwrap_err().contains("rect 重叠"));

        let mut w = sample();
        w.pages[0].components = vec![
            comp(
                "core:value",
                Some(WidgetRectDto {
                    x: 0,
                    y: 0,
                    w: 3,
                    h: 2,
                }),
            ),
            comp("core:value", None),
        ];
        assert!(store.upsert(w).unwrap_err().contains("全部组件都声明"));
    }

    #[test]
    fn remove_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = workspaces_path(tmp.path());
        let mut store = WorkspaceStore::load(&path);
        store.upsert(sample()).unwrap();
        store.remove("com.acme.pm3000").unwrap();
        store.remove("com.acme.pm3000").unwrap();
        assert!(WorkspaceStore::load(&path).list().is_empty());
    }
}
