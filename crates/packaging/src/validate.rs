//! 设备包语义校验：解析成功之后、可安装之前的第二道门。

use crate::manifest::PackageManifest;
use hmi_domain::driver::Driver;
use hmi_domain::points::PointsTable;
use std::sync::Arc;

/// 宿主提供的设备页面组件白名单。设备包只能用这些 kind（或自带 ESM 组件 /
/// `plugin:` 插件组件）；增删核心组件需同步宿主 `ui/src/utils/devicePages.ts`。
/// 新组件使用带命名空间的形式（`core:value`），裸名作为 v1 兼容别名。
pub const ALLOWED_DEVICE_PAGE_COMPONENTS: &[&str] = &[
    "overview",
    "dashboard",
    "tags",
    "settings",
    "value",
    "gauge",
    "switch",
    "input",
    "bits",
    "group",
    "core:value",
    "core:gauge",
    "core:switch",
    "core:input",
    "core:bits",
    "core:group",
    "core:overview",
    "core:dashboard",
    "core:tags",
    "core:settings",
];

/// 设备包允许声明的 ESM 组件权限。
pub const ALLOWED_WIDGET_PERMISSIONS: &[&str] =
    &["tags:read", "tags:subscribe", "tags:write", "notify"];

/// 需要 `value` 槽位的显示/写入型组件：缺省时只提示，不拦截。
const VALUE_SLOT_WIDGET_KINDS: &[&str] = &[
    "value",
    "gauge",
    "switch",
    "input",
    "bits",
    "core:value",
    "core:gauge",
    "core:switch",
    "core:input",
    "core:bits",
];

/// 页面网格列数（与仪表盘一致）。
pub const PAGE_GRID_COLS: u32 = 12;
/// 绝对定位页面的最大行数（防止 y 值异常）。
const MAX_PAGE_ROWS: u32 = 200;

/// 校验一组组件 rect：整页要么全部声明、要么全部省略；越界/重叠均报错。
///
/// `rects` 与页面组件一一对应，`None` 表示该组件走流式布局。
pub fn validate_component_rects(
    page_id: &str,
    rects: &[Option<(u32, u32, u32, u32)>],
) -> Vec<String> {
    let mut errors = Vec::new();
    let declared = rects.iter().filter(|r| r.is_some()).count();
    if declared > 0 && declared != rects.len() {
        errors.push(format!(
            "页面 `{page_id}`：rect 必须全部组件都声明或全部省略（当前 {declared}/{}）",
            rects.len()
        ));
        return errors;
    }
    if declared == 0 {
        return errors;
    }

    let mut placed: Vec<(usize, (u32, u32, u32, u32))> = Vec::new();
    for (i, rect) in rects.iter().enumerate() {
        let Some((x, y, w, h)) = *rect else {
            continue;
        };
        if w == 0 || h == 0 {
            errors.push(format!(
                "页面 `{page_id}`：组件 #{i} 的 rect 宽高必须大于 0"
            ));
            continue;
        }
        if x >= PAGE_GRID_COLS || x + w > PAGE_GRID_COLS {
            errors.push(format!(
                "页面 `{page_id}`：组件 #{i} 的 rect 横向越界（x={x}, w={w}，共 {PAGE_GRID_COLS} 列）"
            ));
        }
        if y + h > MAX_PAGE_ROWS {
            errors.push(format!(
                "页面 `{page_id}`：组件 #{i} 的 rect 纵向越界（y={y}, h={h}）"
            ));
        }
        for (j, (px, py, pw, ph)) in &placed {
            if x < px + pw && *px < x + w && y < py + ph && *py < y + h {
                errors.push(format!(
                    "页面 `{page_id}`：组件 #{i} 与组件 #{j} 的 rect 重叠"
                ));
            }
        }
        placed.push((i, (x, y, w, h)));
    }
    errors
}

const MAX_UI_PAGES: usize = 50;
const MAX_UI_WIDGETS: usize = 16;
const MAX_PAGE_COMPONENTS: usize = 20;
const MAX_PAGE_TITLE: usize = 120;
const MAX_PAGE_ICON: usize = 64;
const MAX_GROUP_ITEMS: usize = 12;

/// 组件 kind 是否被宿主认识：
/// 核心组件（含 `core:` 别名）、本包声明的 ESM 组件（短名或 `<包id>:<短名>`）、
/// `plugin:<插件id>:<组件id>`（插件存在性由宿主渲染时校验）。
pub fn is_allowed_device_component_kind(manifest: &PackageManifest, kind: &str) -> bool {
    if ALLOWED_DEVICE_PAGE_COMPONENTS.contains(&kind) {
        return true;
    }
    if let Some(rest) = kind.strip_prefix("core:") {
        if ALLOWED_DEVICE_PAGE_COMPONENTS.contains(&rest) {
            return true;
        }
    }
    if manifest.ui.widgets.iter().any(|w| w.id == kind) {
        return true;
    }
    if let Some((pkg, local)) = kind.split_once(':') {
        if pkg == manifest.id && manifest.ui.widgets.iter().any(|w| w.id == local) {
            return true;
        }
    }
    if let Some(rest) = kind.strip_prefix("plugin:") {
        let mut parts = rest.split(':');
        return matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(a), Some(b), None) if !a.is_empty() && !b.is_empty()
        );
    }
    false
}

/// 校验结果：errors 非空 = 拒绝；warnings 提示但不拦截。
#[derive(Debug, Default, Clone)]
pub struct ValidationIssues {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationIssues {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }

    fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }
}

/// 校验 manifest + 点表的一致性与合法性。
///
/// `drivers` 是宿主已注册的驱动列表：驱动必须存在，包声明的 transports
/// 必须是驱动能力的子集，点表的协议相关规则由驱动补充校验。
pub fn validate_package(
    manifest: &PackageManifest,
    table: &PointsTable,
    drivers: &[Arc<dyn Driver>],
) -> ValidationIssues {
    let mut normalized = manifest.clone();
    normalized.ui.normalize(&normalized.id);
    let manifest = &normalized;
    let mut v = ValidationIssues::default();
    validate_manifest(manifest, drivers, &mut v);
    validate_ui(manifest, table, &mut v);
    validate_points(table, &mut v);

    if let Some(driver) = drivers.iter().find(|d| d.id() == manifest.protocol.driver) {
        let issues = driver.validate_package(table, &manifest.protocol);
        v.errors.extend(issues.errors);
        v.warnings.extend(issues.warnings);
    }
    v
}

fn validate_manifest(m: &PackageManifest, drivers: &[Arc<dyn Driver>], v: &mut ValidationIssues) {
    // id：小写字母/数字，点或横线分隔，至少两段
    let id = &m.id;
    let lowered = id.to_lowercase();
    let shape_ok = !id.is_empty()
        && *id == lowered
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        && id.split('.').count() >= 2
        && id.split('.').all(|s| !s.is_empty());
    if !shape_ok {
        v.error(format!(
            "manifest.id `{id}` 不合法：应为小写点分形式（如 com.acme.pm3000），至少两段"
        ));
    }
    if m.name.trim().is_empty() {
        v.error("manifest.name 不能为空");
    }
    if m.protocol.driver.trim().is_empty() {
        v.error("manifest.protocol.driver 不能为空");
    } else {
        match drivers.iter().find(|d| d.id() == m.protocol.driver) {
            None => {
                let known: Vec<&str> = drivers.iter().map(|d| d.id()).collect();
                v.error(format!(
                    "protocol.driver `{}` 未注册（宿主支持: {}）",
                    m.protocol.driver,
                    if known.is_empty() {
                        "无".to_string()
                    } else {
                        known.join(", ")
                    }
                ));
            }
            Some(driver) => {
                for t in &m.protocol.transports {
                    if !driver.transports().contains(t) {
                        v.error(format!(
                            "protocol.transports 包含 `{t}`，但驱动 `{}` 只支持：{}",
                            driver.id(),
                            driver
                                .transports()
                                .iter()
                                .map(|x| x.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                }
            }
        }
    }
    if m.schema_version != 2 {
        v.error(format!(
            "不支持的 schemaVersion {}（当前宿主支持 2）",
            m.schema_version
        ));
    }
}

/// 校验设备包声明的默认工作区页面与宿主组件引用。
fn validate_ui(m: &PackageManifest, t: &PointsTable, v: &mut ValidationIssues) {
    validate_widget_decls(m, v);
    if m.ui.pages.len() > MAX_UI_PAGES {
        v.error(format!("ui.pages 最多 {MAX_UI_PAGES} 页"));
    }
    let mut page_ids = std::collections::HashSet::new();
    for page in &m.ui.pages {
        let ctx = format!("ui 页面 `{}`", page.id);
        let page_id_ok = !page.id.is_empty()
            && page.id.len() <= 64
            && page
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !page_id_ok {
            v.error(format!("{ctx}：id 只允许字母/数字/下划线/横线（≤64）"));
        }
        if !page_ids.insert(page.id.as_str()) {
            v.error(format!("{ctx}：id 重复"));
        }
        if page.title.trim().is_empty() {
            v.error(format!("{ctx}：title 不能为空"));
        } else if page.title.chars().count() > MAX_PAGE_TITLE {
            v.error(format!("{ctx}：title 过长（≤{MAX_PAGE_TITLE}）"));
        }
        if page.icon.chars().count() > MAX_PAGE_ICON {
            v.error(format!("{ctx}：icon 过长（≤{MAX_PAGE_ICON}）"));
        }
        match page.schema_version.unwrap_or(1) {
            1 => {
                if !page.widgets.is_empty() {
                    v.warn(format!("{ctx}：schemaVersion 1 下 widgets[] 会被忽略"));
                }
            }
            2 => {
                // 归一化会把 widgets[] 翻成 components[]（widgets 清空），
                // 因此「两者同时非空」才是作者真的混用了两种写法。
                if !page.widgets.is_empty() && !page.components.is_empty() {
                    v.error(format!(
                        "{ctx}：schemaVersion 2 请使用 widgets[]，不要混用 components[]"
                    ));
                }
                if page.widgets.is_empty() && page.components.is_empty() {
                    v.error(format!("{ctx}：schemaVersion 2 页面至少需要一个 widget"));
                }
            }
            other => v.error(format!("{ctx}：不支持的 schemaVersion {other}（1 或 2）")),
        }
        if let Some(layout) = page.layout {
            if layout.cols != PAGE_GRID_COLS {
                v.error(format!(
                    "{ctx}：layout.cols 必须为 {PAGE_GRID_COLS}（宿主固定列数）"
                ));
            }
            if layout.row_height != 72 || layout.gap != 12 {
                v.warn(format!(
                    "{ctx}：宿主固定 rowHeight=72 / gap=12，声明值会被忽略"
                ));
            }
        }
        if page.components.is_empty() {
            v.error(format!("{ctx}：至少需要一个组件"));
        }
        if page.components.len() > MAX_PAGE_COMPONENTS {
            v.error(format!("{ctx}：组件最多 {MAX_PAGE_COMPONENTS} 个"));
        }
        let rects: Vec<Option<(u32, u32, u32, u32)>> = page
            .components
            .iter()
            .map(|c| c.rect.map(|r| (r.x, r.y, r.w, r.h)))
            .collect();
        for e in validate_component_rects(&page.id, &rects) {
            v.error(e);
        }
        let mut comp_ids = std::collections::HashSet::new();
        for comp in &page.components {
            if let Some(id) = &comp.id {
                if !valid_local_id(id) {
                    v.error(format!(
                        "{ctx}：组件实例 id `{id}` 非法（字母/数字/-/_，≤64）"
                    ));
                } else if !comp_ids.insert(id.as_str()) {
                    v.error(format!("{ctx}：组件实例 id `{id}` 重复"));
                }
            }
            if !is_allowed_device_component_kind(m, &comp.kind) {
                v.error(format!(
                    "{ctx}：未知组件 `{}`（可用：核心组件、本包 ui.widgets 声明的组件或 plugin:<插件id>:<组件id>）",
                    comp.kind
                ));
            }
            if comp.title.chars().count() > MAX_PAGE_TITLE {
                v.error(format!("{ctx}：组件 title 过长（≤{MAX_PAGE_TITLE}）"));
            }
            if !comp.config.is_null() && !comp.config.is_object() {
                v.error(format!(
                    "{ctx}：组件 `{}` 的 config 必须是 JSON 对象",
                    comp.kind
                ));
            }
            validate_bindings(&ctx, &comp.kind, &comp.bind, t, v);
            validate_group_config(&ctx, &comp.kind, &comp.config, t, v, 0);
            if VALUE_SLOT_WIDGET_KINDS.contains(&comp.kind.as_str())
                && !comp.bind.contains_key("value")
            {
                v.warn(format!(
                    "{ctx}：组件 `{}` 未绑定 value 槽位（浏览时显示空态）",
                    comp.kind
                ));
            }
        }
    }
}

/// 校验包自带的 ESM 组件声明。
fn validate_widget_decls(m: &PackageManifest, v: &mut ValidationIssues) {
    if m.ui.widgets.len() > MAX_UI_WIDGETS {
        v.error(format!("ui.widgets 最多 {MAX_UI_WIDGETS} 个"));
    }
    let mut ids = std::collections::HashSet::new();
    for w in &m.ui.widgets {
        let ctx = format!("ui.widgets `{}`", w.id);
        if !valid_local_id(&w.id) {
            v.error(format!(
                "{ctx}：id 非法（字母/数字/-/_，≤64，且须以字母或数字开头）"
            ));
        }
        if !ids.insert(w.id.as_str()) {
            v.error(format!("{ctx}：id 重复"));
        }
        if ALLOWED_DEVICE_PAGE_COMPONENTS.contains(&w.id.as_str()) {
            v.error(format!("{ctx}：id 与核心组件同名，请改名"));
        }
        if w.title.trim().is_empty() {
            v.error(format!("{ctx}：title 不能为空"));
        }
        let entry = std::path::Path::new(&w.entry);
        let entry_ok = !w.entry.trim().is_empty()
            && !entry.is_absolute()
            && entry
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)));
        if !entry_ok || !w.entry.replace('\\', "/").starts_with("widgets/") {
            v.error(format!(
                "{ctx}：entry 必须是 widgets/ 下的相对路径，当前 `{}`",
                w.entry
            ));
        } else {
            let ext = entry
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext != "js" && ext != "mjs" {
                v.error(format!("{ctx}：entry 必须是 ESM 脚本（.js / .mjs）"));
            }
        }
        for p in &w.permissions {
            if !ALLOWED_WIDGET_PERMISSIONS.contains(&p.as_str()) {
                v.error(format!(
                    "{ctx}：未知权限 `{p}`（可用：{}）",
                    ALLOWED_WIDGET_PERMISSIONS.join(", ")
                ));
            }
        }
        if w.size.w == 0 || w.size.w > PAGE_GRID_COLS || w.size.h == 0 || w.size.h > MAX_PAGE_ROWS {
            v.error(format!(
                "{ctx}：size.w 需在 1–{PAGE_GRID_COLS}，size.h 需 ≥ 1"
            ));
        }
        if w.size.min_w == 0 || w.size.min_w > w.size.w {
            v.error(format!("{ctx}：minW 需在 1–w 之间"));
        }
        if w.size.min_h == 0 || w.size.min_h > w.size.h {
            v.error(format!("{ctx}：minH 需在 1–h 之间"));
        }
        for (slot, b) in &w.bindings {
            if !valid_local_id(slot) {
                v.error(format!("{ctx}：槽位名 `{slot}` 非法"));
            }
            if !matches!(b.kind.as_str(), "point" | "value" | "text") {
                v.error(format!(
                    "{ctx}：槽位 `{slot}` 的 type 必须是 point / value / text"
                ));
            }
        }
    }
}

/// 校验 `core:group` 的声明式子组件。
fn validate_group_config(
    ctx: &str,
    kind: &str,
    config: &serde_json::Value,
    t: &PointsTable,
    v: &mut ValidationIssues,
    depth: usize,
) {
    let normalized = kind.strip_prefix("core:").unwrap_or(kind);
    if normalized != "group" {
        return;
    }
    if depth > 1 {
        v.error(format!("{ctx}：core:group 最多嵌套一层"));
        return;
    }
    let Some(items) = config.get("items").and_then(|x| x.as_array()) else {
        v.error(format!("{ctx}：core:group 的 config.items 必须是数组"));
        return;
    };
    if items.is_empty() {
        v.error(format!("{ctx}：core:group 至少需要一个子组件"));
    }
    if items.len() > MAX_GROUP_ITEMS {
        v.error(format!("{ctx}：core:group 子组件最多 {MAX_GROUP_ITEMS} 个"));
    }
    let mut rects = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let ictx = format!("{ctx} 的子组件 #{i}");
        let Some(obj) = item.as_object() else {
            v.error(format!("{ictx}：必须是 JSON 对象"));
            continue;
        };
        let child_kind = obj.get("kind").and_then(|x| x.as_str()).unwrap_or("");
        if child_kind.is_empty() {
            v.error(format!("{ictx}：缺少 kind"));
        } else if child_kind == kind || child_kind == "group" {
            v.error(format!("{ictx}：core:group 不能直接嵌套自身"));
        }
        let rect = obj.get("rect").and_then(|r| {
            Some((
                r.get("x")?.as_u64()? as u32,
                r.get("y")?.as_u64()? as u32,
                r.get("w")?.as_u64()? as u32,
                r.get("h")?.as_u64()? as u32,
            ))
        });
        if rect.is_none() {
            v.error(format!("{ictx}：缺少 rect（组内网格位置）"));
        }
        rects.push(rect);
        if let Some(bind) = obj.get("bind") {
            match serde_json::from_value::<
                std::collections::BTreeMap<String, crate::manifest::BindingSpec>,
            >(bind.clone())
            {
                Ok(b) => validate_bindings(&ictx, child_kind, &b, t, v),
                Err(e) => v.error(format!("{ictx}：bind 解析失败：{e}")),
            }
        }
        if let Some(child_config) = obj.get("config") {
            validate_group_config(&ictx, child_kind, child_config, t, v, depth + 1);
        }
    }
    for e in validate_component_rects("core:group", &rects) {
        v.error(format!("{ctx}：{e}"));
    }
}

/// 校验页面组件的绑定：包内只允许相对点绑定，点/位必须存在于点表。
fn validate_bindings(
    ctx: &str,
    comp_kind: &str,
    bind: &std::collections::BTreeMap<String, crate::manifest::BindingSpec>,
    t: &PointsTable,
    v: &mut ValidationIssues,
) {
    for (slot, spec) in bind {
        let bctx = format!("{ctx}：组件 `{comp_kind}` 槽位 `{slot}`");
        if slot.trim().is_empty() {
            v.error(format!("{ctx}：组件 `{comp_kind}` 存在空槽位名"));
            continue;
        }
        let decls = spec.decls();
        if decls.is_empty() {
            v.error(format!("{bctx}：绑定数组为空"));
            continue;
        }
        for b in decls {
            validate_one_binding(&bctx, comp_kind, b, t, v);
        }
    }
}

fn validate_one_binding(
    bctx: &str,
    comp_kind: &str,
    b: &crate::manifest::BindingDecl,
    t: &PointsTable,
    v: &mut ValidationIssues,
) {
    let has_point = b.point.as_deref().is_some_and(|s| !s.trim().is_empty());
    let has_tag = b.tag.as_deref().is_some_and(|s| !s.trim().is_empty());
    if has_point == has_tag {
        v.error(format!("{bctx}：必须且只能声明 point 或 tag 之一"));
        return;
    }
    if has_tag {
        // core:group 的文本槽位允许任意字符串，但点绑定仍必须是相对的。
        if matches!(comp_kind, "core:text" | "text") {
            return;
        }
        v.error(format!(
            "{bctx}：设备包页面不允许绝对 tag 绑定（跨设备绑定仅用户页面可用）"
        ));
        return;
    }
    let point_id = b.point.as_deref().unwrap_or_default();
    let Some(point) = t.point(point_id) else {
        v.error(format!("{bctx}：点 `{point_id}` 不在点表中"));
        return;
    };
    if let Some(bit) = b.bit {
        if point.is_bit_point() {
            v.error(format!("{bctx}：位区域点 `{point_id}` 不支持 bit 绑定"));
        } else if !point.bits.contains_key(&bit) {
            v.error(format!("{bctx}：点 `{point_id}` 没有位 {bit} 的定义"));
        }
    }
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

fn validate_points(t: &PointsTable, v: &mut ValidationIssues) {
    // 轮询组
    let mut group_defined: Vec<&str> = Vec::new();
    for g in &t.poll_groups {
        if g.name.trim().is_empty() {
            v.error("轮询组 name 不能为空");
            continue;
        }
        if g.interval_ms == 0 {
            v.error(format!("轮询组 `{}` intervalMs 必须大于 0", g.name));
        } else if g.interval_ms < 50 {
            v.warn(format!(
                "轮询组 `{}` intervalMs={} 过短，485 总线可能跑不满",
                g.name, g.interval_ms
            ));
        }
        group_defined.push(&g.name);
    }

    let mut referenced: Vec<&str> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for p in &t.points {
        let ctx = format!("点 `{}`", p.id);
        if p.id.trim().is_empty() {
            v.error("存在空点 id");
            continue;
        }
        if !p
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            v.error(format!("{ctx}：id 只允许字母/数字/下划线/横线"));
        }
        if !seen.insert(&p.id) {
            v.error(format!("{ctx}：id 重复"));
        }

        // 轮询组引用
        if let Some(g) = &p.poll_group {
            if !group_defined.contains(&g.as_str()) {
                v.error(format!("{ctx}：引用了未定义的轮询组 `{g}`"));
            }
            referenced.push(g);
        } else {
            referenced.push("default");
        }

        if p.scale == Some(0.0) {
            v.error(format!("{ctx}：scale 为 0（非法换算）"));
        }

        // 位展开是通用元数据；地址/类型是否支持由驱动校验
        for (&bit, def) in &p.bits {
            if bit >= 16 {
                v.error(format!("{ctx}：位号 {bit} 超出 0-15"));
            }
            if def.name.trim().is_empty() {
                v.error(format!("{ctx}：b{bit} 的 name 为空"));
            }
        }

        // 写值元数据：边界/步长/枚举都按工程量口径解释
        if let (Some(lo), Some(hi)) = (p.min, p.max) {
            if lo > hi {
                v.error(format!("{ctx}：min({lo}) 不能大于 max({hi})"));
            }
        }
        if p.min.is_some_and(|x| !x.is_finite()) || p.max.is_some_and(|x| !x.is_finite()) {
            v.error(format!("{ctx}：min/max 必须是有限数值"));
        }
        if let Some(step) = p.step {
            if !step.is_finite() || step <= 0.0 {
                v.error(format!("{ctx}：step 必须大于 0"));
            }
        }
        if !p.value_enum.is_empty() {
            if !p.data_type.is_integer() {
                v.error(format!(
                    "{ctx}：enum 仅支持整数类型（当前 {}）",
                    p.data_type.as_str()
                ));
            }
            let mut enum_values = std::collections::HashSet::new();
            for e in &p.value_enum {
                if e.label.trim().is_empty() {
                    v.error(format!("{ctx}：enum 项 value={} 的 label 为空", e.value));
                }
                if !enum_values.insert(e.value) {
                    v.error(format!("{ctx}：enum value={} 重复", e.value));
                }
                let ev = e.value as f64;
                if p.min.is_some_and(|lo| ev < lo) || p.max.is_some_and(|hi| ev > hi) {
                    v.error(format!("{ctx}：enum value={} 超出 [min,max]", e.value));
                }
            }
        }
    }

    // 定义了但无人引用的组 → 提示
    for g in &group_defined {
        if !referenced.contains(g) {
            v.warn(format!("轮询组 `{g}` 没有任何点引用"));
        }
    }
    if !t.points.is_empty() && t.poll_groups.is_empty() {
        v.warn("未定义轮询组，全部点将进入默认组（1000ms）");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PackageManifest;

    fn test_drivers() -> Vec<Arc<dyn Driver>> {
        vec![Arc::new(hmi_driver_modbus::ModbusDriver::new())]
    }

    fn manifest() -> PackageManifest {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 2,
            "id": "com.acme.pm3000",
            "name": "PM3000",
            "version": "1.0.0",
            "protocol": { "driver": "modbus" }
        }))
        .unwrap()
    }

    fn points_json(v: serde_json::Value) -> PointsTable {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn valid_package_passes() {
        let t = points_json(serde_json::json!({
            "pollGroups": [{ "name": "fast", "intervalMs": 1000 }],
            "points": [
                { "id": "voltage", "address": "40101", "type": "u16", "scale": 0.1, "pollGroup": "fast" }
            ]
        }));
        let v = validate_package(&manifest(), &t, &test_drivers());
        assert!(v.ok(), "{v:?}");
        assert!(v.warnings.is_empty());
    }

    #[test]
    fn catches_duplicates_and_bad_group() {
        let t = points_json(serde_json::json!({
            "pollGroups": [{ "name": "fast", "intervalMs": 0 }],
            "points": [
                { "id": "a", "address": "40101", "pollGroup": "fast" },
                { "id": "a", "address": "40102", "pollGroup": "nope" }
            ]
        }));
        let v = validate_package(&manifest(), &t, &test_drivers());
        assert!(!v.ok());
        assert!(v.errors.iter().any(|e| e.contains("id 重复")));
        assert!(v.errors.iter().any(|e| e.contains("未定义的轮询组")));
        assert!(v.errors.iter().any(|e| e.contains("intervalMs 必须大于 0")));
    }

    #[test]
    fn catches_readonly_write() {
        let t = points_json(serde_json::json!({
            "points": [
                { "id": "ir", "address": "30100", "type": "u16", "access": "rw" }
            ]
        }));
        let v = validate_package(&manifest(), &t, &test_drivers());
        assert!(v.errors.iter().any(|e| e.contains("只读")));
    }

    #[test]
    fn catches_bad_bits() {
        let t = points_json(serde_json::json!({
            "points": [
                { "id": "st", "address": "40300", "type": "u32",
                  "bits": { "20": { "name": "x" } } }
            ]
        }));
        let v = validate_package(&manifest(), &t, &test_drivers());
        assert!(v.errors.iter().any(|e| e.contains("bits")));
    }

    #[test]
    fn warns_missing_byte_order_multiword() {
        let t = points_json(serde_json::json!({
            "points": [ { "id": "e", "address": "40200", "type": "u32" } ]
        }));
        let v = validate_package(&manifest(), &t, &test_drivers());
        assert!(v.ok());
        assert!(v.warnings.iter().any(|w| w.contains("byteOrder")));
    }

    #[test]
    fn unknown_driver_rejected() {
        let mut m = manifest();
        m.protocol.driver = "does-not-exist".into();
        let v = validate_package(&m, &PointsTable::default(), &test_drivers());
        assert!(v.errors.iter().any(|e| e.contains("未注册")), "{v:?}");
    }

    #[test]
    fn package_transport_must_be_supported() {
        let mut m = manifest();
        m.protocol.transports = vec![hmi_domain::link::TransportKind::Ble];
        let v = validate_package(&m, &PointsTable::default(), &test_drivers());
        assert!(v.errors.iter().any(|e| e.contains("transports")), "{v:?}");
    }

    #[test]
    fn bad_manifest_id_rejected() {
        let mut m = manifest();
        m.id = "PM 3000".into();
        let v = validate_package(&m, &PointsTable::default(), &test_drivers());
        assert!(v.errors.iter().any(|e| e.contains("manifest.id")));
    }

    #[test]
    fn ui_pages_validated() {
        let mut m = manifest();
        m.ui = serde_json::from_value(serde_json::json!({
            "pages": [
                { "id": "overview", "title": "设备概览",
                  "components": [{ "kind": "overview" }] },
                { "id": "overview", "title": "重复页",
                  "components": [{ "kind": "shell" }] },
                { "id": "empty", "title": "空页", "components": [] }
            ]
        }))
        .unwrap();
        let v = validate_package(&m, &PointsTable::default(), &test_drivers());
        assert!(v.errors.iter().any(|e| e.contains("id 重复")));
        assert!(v.errors.iter().any(|e| e.contains("未知组件")));
        assert!(v.errors.iter().any(|e| e.contains("至少需要一个组件")));
    }

    #[test]
    fn ui_pages_optional_and_valid() {
        let mut m = manifest();
        m.ui = serde_json::from_value(serde_json::json!({
            "pages": [
                { "id": "dashboard", "title": "仪表盘", "icon": "layout",
                  "components": [{ "kind": "dashboard" }, { "kind": "tags" }] }
            ]
        }))
        .unwrap();
        let v = validate_package(&m, &PointsTable::default(), &test_drivers());
        assert!(v.ok(), "{v:?}");
    }

    #[test]
    fn ui_relative_bindings_validated() {
        let t = points_json(serde_json::json!({
            "points": [
                { "id": "uab", "address": "40101", "type": "u16" },
                { "id": "status", "address": "40300", "type": "u16",
                  "bits": { "0": { "name": "运行" } } }
            ]
        }));
        let mut m = manifest();
        m.ui = serde_json::from_value(serde_json::json!({
            "pages": [
                { "id": "monitor", "title": "监测", "components": [
                    { "kind": "core:value", "bind": { "value": { "point": "uab" } } },
                    { "kind": "core:value", "bind": { "value": { "point": "status", "bit": 0 } } },
                    { "kind": "core:gauge", "bind": { "value": { "point": "nope" } } },
                    { "kind": "core:value", "bind": { "value": { "tag": "pm1.uab" } } },
                    { "kind": "core:value", "bind": { "value": { "point": "status", "bit": 3 } } }
                ] }
            ]
        }))
        .unwrap();
        let v = validate_package(&m, &t, &test_drivers());
        assert!(!v.ok());
        assert!(v.errors.iter().any(|e| e.contains("不在点表")), "{v:?}");
        assert!(v.errors.iter().any(|e| e.contains("绝对 tag")), "{v:?}");
        assert!(v.errors.iter().any(|e| e.contains("没有位 3")), "{v:?}");
    }

    #[test]
    fn write_metadata_validated() {
        let t = points_json(serde_json::json!({
            "points": [
                { "id": "mode", "address": "40311", "type": "u16", "access": "rw",
                  "min": 0, "max": 2, "step": 1,
                  "enum": [ { "value": 0, "label": "自动" }, { "value": 1, "label": "手动" } ] },
                { "id": "bad", "address": "40312", "type": "f32", "access": "rw",
                  "min": 5, "max": 1, "step": 0,
                  "enum": [ { "value": 0, "label": "" } ] },
                { "id": "coil", "address": "00010", "min": 0 }
            ]
        }));
        let v = validate_package(&manifest(), &t, &test_drivers());
        assert!(!v.ok());
        assert!(
            v.errors
                .iter()
                .any(|e| e.contains("min(5) 不能大于 max(1)")),
            "{v:?}"
        );
        assert!(
            v.errors.iter().any(|e| e.contains("step 必须大于 0")),
            "{v:?}"
        );
        assert!(
            v.errors.iter().any(|e| e.contains("enum 仅支持整数类型")),
            "{v:?}"
        );
        assert!(v.errors.iter().any(|e| e.contains("label 为空")), "{v:?}");
        assert!(v.errors.iter().any(|e| e.contains("位区域点")), "{v:?}");
    }

    #[test]
    fn component_rects_validated() {
        let mut m = manifest();
        m.ui = serde_json::from_value(serde_json::json!({
            "pages": [
                { "id": "grid", "title": "网格", "components": [
                    { "kind": "core:value", "rect": { "x": 0, "y": 0, "w": 3, "h": 2 } },
                    { "kind": "core:value", "rect": { "x": 3, "y": 0, "w": 3, "h": 2 } }
                ] },
                { "id": "partial", "title": "半网格", "components": [
                    { "kind": "core:value", "rect": { "x": 0, "y": 0, "w": 3, "h": 2 } },
                    { "kind": "core:value" }
                ] },
                { "id": "oob", "title": "越界", "components": [
                    { "kind": "core:value", "rect": { "x": 10, "y": 0, "w": 4, "h": 2 } }
                ] },
                { "id": "overlap", "title": "重叠", "components": [
                    { "kind": "core:value", "rect": { "x": 0, "y": 0, "w": 6, "h": 2 } },
                    { "kind": "core:gauge", "rect": { "x": 3, "y": 0, "w": 6, "h": 2 } }
                ] }
            ]
        }))
        .unwrap();
        let v = validate_package(&m, &PointsTable::default(), &test_drivers());
        assert_eq!(v.errors.len(), 3, "{v:?}");
        assert!(
            v.errors.iter().any(|e| e.contains("全部组件都声明")),
            "{v:?}"
        );
        assert!(v.errors.iter().any(|e| e.contains("横向越界")), "{v:?}");
        assert!(v.errors.iter().any(|e| e.contains("rect 重叠")), "{v:?}");
        assert!(!v.errors.iter().any(|e| e.contains("`grid`")), "{v:?}");
    }

    #[test]
    fn valid_write_metadata_passes() {
        let t = points_json(serde_json::json!({
            "points": [
                { "id": "mode", "address": "40311", "type": "u16", "access": "rw",
                  "min": 0, "max": 2, "step": 1,
                  "enum": [ { "value": 0, "label": "自动" }, { "value": 2, "label": "停止" } ] }
            ]
        }));
        let v = validate_package(&manifest(), &t, &test_drivers());
        assert!(v.ok(), "{v:?}");
    }

    /// P3：设备包 ESM 组件声明的 id/entry/权限/尺寸都要校验。
    #[test]
    fn widget_declarations_validated() {
        let mut m = manifest();
        m.ui = serde_json::from_value(serde_json::json!({
            "widgets": [
                { "id": "trend", "title": "趋势", "entry": "widgets/trend.js",
                  "bindings": { "series": { "type": "point", "many": true } },
                  "permissions": ["tags:read", "nets:evil"] },
                { "id": "trend", "title": "", "entry": "../x.js", "permissions": [] },
                { "id": "value", "title": "撞名", "entry": "widgets/v.js" },
                { "id": "bad-size", "title": "尺寸", "entry": "widgets/s.js",
                  "size": { "w": 0, "h": 2, "minW": 3, "minH": 0 } }
            ],
            "pages": []
        }))
        .unwrap();
        let v = validate_package(&m, &PointsTable::default(), &test_drivers());
        assert!(!v.ok());
        assert!(v.errors.iter().any(|e| e.contains("id 重复")), "{v:?}");
        assert!(
            v.errors.iter().any(|e| e.contains("与核心组件同名")),
            "{v:?}"
        );
        assert!(
            v.errors.iter().any(|e| e.contains("entry 必须是 widgets/")),
            "{v:?}"
        );
        assert!(v.errors.iter().any(|e| e.contains("未知权限")), "{v:?}");
        assert!(
            v.errors.iter().any(|e| e.contains("title 不能为空")),
            "{v:?}"
        );
        assert!(v.errors.iter().any(|e| e.contains("minW")), "{v:?}");
    }

    /// P4：v2 页面（widgets[]）、包内组件短名与 core:group 组合的校验。
    #[test]
    fn v2_widgets_and_group_validated() {
        let t = points_json(serde_json::json!({
            "points": [ { "id": "uab", "address": "40101", "type": "u16" } ]
        }));
        let mut m = manifest();
        m.ui = serde_json::from_value(serde_json::json!({
            "widgets": [
                { "id": "trend", "title": "趋势", "entry": "widgets/trend.js",
                  "bindings": { "series": { "type": "point", "many": true } } }
            ],
            "pages": [
                { "schemaVersion": 2, "id": "trend", "title": "趋势",
                  "layout": { "cols": 12, "rowHeight": 72, "gap": 12 },
                  "widgets": [
                    { "id": "w1", "kind": "trend", "rect": { "x": 0, "y": 0, "w": 6, "h": 3 },
                      "bind": { "series": [ { "point": "uab" }, { "point": "nope" } ] } },
                    { "id": "w1", "kind": "core:group", "rect": { "x": 6, "y": 0, "w": 6, "h": 4 },
                      "config": { "items": [
                          { "kind": "core:value", "rect": { "x": 0, "y": 0, "w": 12, "h": 2 },
                            "bind": { "value": { "point": "uab" } } },
                          { "kind": "core:group", "rect": { "x": 0, "y": 2, "w": 12, "h": 2 } }
                      ] } }
                  ] }
            ]
        }))
        .unwrap();
        let v = validate_package(&m, &t, &test_drivers());
        assert!(!v.ok());
        assert!(v.errors.iter().any(|e| e.contains("不在点表")), "{v:?}");
        // 组件实例 id 重复
        assert!(v.errors.iter().any(|e| e.contains("实例 id")), "{v:?}");
        // 组合容器不允许直接嵌套自身
        assert!(v.errors.iter().any(|e| e.contains("嵌套自身")), "{v:?}");
    }

    /// 合法 v2 页面 + 包内组件应零错误。
    #[test]
    fn v2_page_passes_validation() {
        let t = points_json(serde_json::json!({
            "points": [ { "id": "uab", "address": "40101", "type": "u16" } ]
        }));
        let mut m = manifest();
        m.ui = serde_json::from_value(serde_json::json!({
            "widgets": [
                { "id": "trend", "title": "趋势", "entry": "widgets/trend.js",
                  "bindings": { "series": { "type": "point", "many": true } } }
            ],
            "pages": [
                { "schemaVersion": 2, "id": "main", "title": "主页",
                  "layout": { "cols": 12, "rowHeight": 72, "gap": 12 },
                  "widgets": [
                    { "id": "w1", "kind": "trend", "rect": { "x": 0, "y": 0, "w": 12, "h": 4 },
                      "bind": { "series": [ { "point": "uab" } ] } }
                  ] }
            ]
        }))
        .unwrap();
        let v = validate_package(&m, &t, &test_drivers());
        assert!(v.ok(), "{v:?}");
    }
}
