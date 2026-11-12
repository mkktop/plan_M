//! 仪表盘布局持久化：`<data-dir>/dashboards.json`。
//!
//! 布局属于**工程数据**而不是 UI 本地状态：桌面 UI 可以重装/换机，布局必须还在。
//! 文件格式带 `version`，后续迁移有落脚点；写入走 tmp + rename，避免半截文件。

use hmi_wire::DashboardDto;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: u32 = 1;
const MAX_COLS: u32 = 48;
const MAX_ROWS: u32 = 500;

#[derive(Debug, Serialize, Deserialize)]
struct DashboardFile {
    #[serde(default = "schema_version")]
    version: u32,
    #[serde(default)]
    dashboards: Vec<DashboardDto>,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

/// 仪表盘存储。启动时读盘，损坏文件不阻塞服务（记日志后按空处理）。
pub struct DashboardStore {
    path: PathBuf,
    dashboards: Vec<DashboardDto>,
}

impl DashboardStore {
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let dashboards = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<DashboardFile>(&text) {
                Ok(file) => file.dashboards,
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "仪表盘文件解析失败，按空布局启动（原文件不会被覆盖，直到下次保存）"
                    );
                    Vec::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "读取仪表盘文件失败");
                Vec::new()
            }
        };
        Self { path, dashboards }
    }

    pub fn list(&self) -> Vec<DashboardDto> {
        self.dashboards.clone()
    }

    pub fn get(&self, id: &str) -> Option<&DashboardDto> {
        self.dashboards.iter().find(|d| d.id == id)
    }

    /// 新建或覆盖同 id 仪表盘，成功后落盘。
    pub fn upsert(&mut self, dashboard: DashboardDto) -> Result<(), String> {
        validate_dashboard(&dashboard)?;
        match self.dashboards.iter_mut().find(|d| d.id == dashboard.id) {
            Some(slot) => *slot = dashboard,
            None => self.dashboards.push(dashboard),
        }
        self.persist()
    }

    /// 删除并落盘；不存在视为成功（幂等）。
    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let before = self.dashboards.len();
        self.dashboards.retain(|d| d.id != id);
        if self.dashboards.len() == before {
            return Ok(());
        }
        self.persist()
    }

    fn persist(&self) -> Result<(), String> {
        let file = DashboardFile {
            version: SCHEMA_VERSION,
            dashboards: self.dashboards.clone(),
        };
        let text =
            serde_json::to_string_pretty(&file).map_err(|e| format!("序列化仪表盘失败：{e}"))?;
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

/// 布局语义校验：挡住越界/重复 id 等会破坏画布的输入（组件类型不做白名单，留给插件机制）。
pub fn validate_dashboard(d: &DashboardDto) -> Result<(), String> {
    let id = d.id.trim();
    if id.is_empty() {
        return Err("仪表盘 id 不能为空".into());
    }
    if d.id.len() > 64
        || !d
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("仪表盘 id 只允许字母、数字、下划线、连字符（≤64）".into());
    }
    if d.name.trim().is_empty() {
        return Err("仪表盘名称不能为空".into());
    }
    if d.name.len() > 120 {
        return Err("仪表盘名称过长（≤120）".into());
    }
    if d.cols == 0 || d.cols > MAX_COLS {
        return Err(format!("网格列数需在 1–{MAX_COLS}"));
    }
    match (&d.scope_kind, &d.scope_id) {
        (None, None) => {}
        (Some(kind), Some(id)) => {
            if kind != "device" {
                return Err(format!("未知的看板作用域 `{kind}`（当前支持 device）"));
            }
            if id.is_empty()
                || id.len() > 64
                || !id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                return Err(format!("看板作用域 id `{id}` 非法"));
            }
        }
        _ => return Err("scopeKind 与 scopeId 必须成对出现".into()),
    }

    let mut seen = HashSet::new();
    let mut placements: Vec<&hmi_wire::WidgetLayout> = Vec::new();
    for w in &d.widgets {
        if w.id.trim().is_empty() {
            return Err("组件 id 不能为空".into());
        }
        if !seen.insert(w.id.as_str()) {
            return Err(format!("组件 id `{}` 重复", w.id));
        }
        if w.kind.trim().is_empty() {
            return Err(format!("组件 `{}` 缺少 kind", w.id));
        }
        let l = &w.layout;
        if l.w == 0 || l.h == 0 {
            return Err(format!("组件 `{}` 的宽高必须 ≥1", w.id));
        }
        if l.x + l.w > d.cols {
            return Err(format!(
                "组件 `{}` 超出网格：x={} w={} 超过 {} 列",
                w.id, l.x, l.w, d.cols
            ));
        }
        if l.y + l.h > MAX_ROWS {
            return Err(format!("组件 `{}` 的纵向位置超出上限", w.id));
        }
        if let Some(hit) = placements
            .iter()
            .find(|o| l.x < o.x + o.w && o.x < l.x + l.w && l.y < o.y + o.h && o.y < l.y + l.h)
        {
            return Err(format!(
                "组件 `{}` 与已有组件重叠（x={} y={} w={} h={}）",
                w.id, hit.x, hit.y, hit.w, hit.h
            ));
        }
        placements.push(l);
    }
    Ok(())
}

/// 默认存储路径。
pub fn dashboards_path(data_dir: &Path) -> PathBuf {
    data_dir.join("dashboards.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmi_wire::{WidgetDto, WidgetLayout};

    fn sample(id: &str) -> DashboardDto {
        DashboardDto {
            id: id.into(),
            name: "总览".into(),
            cols: 12,
            scope_kind: None,
            scope_id: None,
            widgets: vec![WidgetDto {
                id: "w1".into(),
                kind: "value".into(),
                title: "电压".into(),
                tag: "pm1.uab".into(),
                unit: "V".into(),
                config: serde_json::json!({ "decimals": 1 }),
                layout: WidgetLayout {
                    x: 0,
                    y: 0,
                    w: 3,
                    h: 2,
                },
            }],
        }
    }

    #[test]
    fn save_and_reload_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = dashboards_path(tmp.path());
        let mut store = DashboardStore::load(&path);
        assert!(store.list().is_empty());

        store.upsert(sample("main")).unwrap();
        assert_eq!(store.list().len(), 1);

        let reloaded = DashboardStore::load(&path);
        let list = reloaded.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0], sample("main"));

        // 同 id 覆盖，不产生第二个条目
        let mut changed = sample("main");
        changed.name = "改过的名字".into();
        store.upsert(changed.clone()).unwrap();
        assert_eq!(store.list().len(), 1);
        assert_eq!(
            DashboardStore::load(&path).get("main").unwrap().name,
            "改过的名字"
        );
    }

    #[test]
    fn invalid_layout_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let mut store = DashboardStore::load(dashboards_path(tmp.path()));

        let mut d = sample("main");
        d.widgets[0].layout.x = 11;
        assert!(store.upsert(d).is_err());

        let mut d = sample("main");
        d.widgets[0].layout.w = 0;
        assert!(store.upsert(d).is_err());

        let mut d = sample("main");
        d.widgets.push(d.widgets[0].clone());
        assert!(store.upsert(d).is_err());

        // 两个组件不允许占据同一网格（否则 UI 会视觉重叠）
        let mut d = sample("main");
        let mut second = d.widgets[0].clone();
        second.id = "w2".into();
        second.layout.x = 1; // 与 w1（0,0,3,2）重叠
        d.widgets.push(second);
        let err = store.upsert(d).unwrap_err();
        assert!(err.contains("重叠"), "{err}");

        let mut d = sample("has space");
        d.id = "bad id".into();
        assert!(store.upsert(d).is_err());

        // scope 成对出现，且只支持 device
        let mut d = sample("scoped");
        d.scope_kind = Some("device".into());
        d.scope_id = Some("pm1".into());
        assert!(store.upsert(d).is_ok());

        let mut d = sample("scoped-bad");
        d.scope_kind = Some("device".into());
        assert!(store.upsert(d).is_err());
    }

    #[test]
    fn malformed_file_is_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let path = dashboards_path(tmp.path());
        std::fs::write(&path, "{ not json").unwrap();
        let store = DashboardStore::load(&path);
        assert!(store.list().is_empty());
    }

    #[test]
    fn remove_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = dashboards_path(tmp.path());
        let mut store = DashboardStore::load(&path);
        store.upsert(sample("main")).unwrap();
        store.remove("main").unwrap();
        store.remove("main").unwrap();
        assert!(DashboardStore::load(&path).list().is_empty());
    }
}
