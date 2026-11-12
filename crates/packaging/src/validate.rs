//! 设备包语义校验：解析成功之后、可安装之前的第二道门。

use crate::manifest::PackageManifest;
use hmi_domain::points::PointsTable;

/// 宿主提供的设备页面组件白名单。设备包只能用这些 kind；
/// 增删组件需同步宿主 `ui/src/utils/devicePages.ts` 与 `workspaces.rs` 校验。
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
    "core:value",
    "core:gauge",
    "core:switch",
    "core:input",
    "core:bits",
];

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
const MAX_PAGE_COMPONENTS: usize = 20;
const MAX_PAGE_TITLE: usize = 120;
const MAX_PAGE_ICON: usize = 64;

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
pub fn validate_package(manifest: &PackageManifest, table: &PointsTable) -> ValidationIssues {
    let mut v = ValidationIssues::default();
    validate_manifest(manifest, &mut v);
    validate_ui(manifest, table, &mut v);
    validate_points(table, &mut v);
    v
}

fn validate_manifest(m: &PackageManifest, v: &mut ValidationIssues) {
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
    }
    if m.schema_version != 1 {
        v.error(format!(
            "不支持的 schemaVersion {}（当前宿主支持 1）",
            m.schema_version
        ));
    }
}

/// 校验设备包声明的默认工作区页面与宿主组件引用。
fn validate_ui(m: &PackageManifest, t: &PointsTable, v: &mut ValidationIssues) {
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
        for comp in &page.components {
            if !ALLOWED_DEVICE_PAGE_COMPONENTS.contains(&comp.kind.as_str()) {
                v.error(format!(
                    "{ctx}：未知组件 `{}`（可用：{}）",
                    comp.kind,
                    ALLOWED_DEVICE_PAGE_COMPONENTS.join(", ")
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
            validate_bindings(&ctx, comp, t, v);
            if VALUE_SLOT_WIDGET_KINDS.contains(&comp.kind.as_str())
                && comp.bind.get("value").is_none()
            {
                v.warn(format!(
                    "{ctx}：组件 `{}` 未绑定 value 槽位（浏览时显示空态）",
                    comp.kind
                ));
            }
        }
    }
}

/// 校验页面组件的绑定：包内只允许相对点绑定，点/位必须存在于点表。
fn validate_bindings(
    ctx: &str,
    comp: &crate::manifest::UiComponentDecl,
    t: &PointsTable,
    v: &mut ValidationIssues,
) {
    for (slot, b) in &comp.bind {
        let bctx = format!("{ctx}：组件 `{}` 槽位 `{slot}`", comp.kind);
        if slot.trim().is_empty() {
            v.error(format!("{ctx}：组件 `{}` 存在空槽位名", comp.kind));
            continue;
        }
        let has_point = b.point.as_deref().is_some_and(|s| !s.trim().is_empty());
        let has_tag = b.tag.as_deref().is_some_and(|s| !s.trim().is_empty());
        if has_point == has_tag {
            v.error(format!("{bctx}：必须且只能声明 point 或 tag 之一"));
            continue;
        }
        if has_tag {
            v.error(format!(
                "{bctx}：设备包页面不允许绝对 tag 绑定（跨设备绑定仅用户页面可用）"
            ));
            continue;
        }
        let point_id = b.point.as_deref().unwrap_or_default();
        let Some(point) = t.point(point_id) else {
            v.error(format!("{bctx}：点 `{point_id}` 不在点表中"));
            continue;
        };
        if let Some(bit) = b.bit {
            if point.is_bit_point() {
                v.error(format!("{bctx}：位区域点 `{point_id}` 不支持 bit 绑定"));
            } else if !point.bits.contains_key(&bit) {
                v.error(format!("{bctx}：点 `{point_id}` 没有位 {bit} 的定义"));
            }
        }
    }
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

        let wc = p.data_type.word_count() as u32;
        let reg = &p.register;

        // 位区域（Coil/Discrete）
        if reg.area.is_bit_area() {
            if !p.bits.is_empty() {
                v.error(format!("{ctx}：位区域点不支持 bits 展开"));
            }
        } else {
            // 字区域
            if reg.offset as u32 + wc > 65536 {
                v.error(format!(
                    "{ctx}：地址 {} + {} 字超出 16 位地址空间",
                    reg.to_engineering(),
                    wc
                ));
            }
            // bits 只允许单字类型
            if !p.bits.is_empty() {
                if wc != 1 {
                    v.error(format!(
                        "{ctx}：bits 只支持 16 位类型（当前 {}）",
                        p.data_type.as_str()
                    ));
                }
                for (&bit, def) in &p.bits {
                    if bit >= 16 {
                        v.error(format!("{ctx}：位号 {bit} 超出 0-15"));
                    }
                    if def.name.trim().is_empty() {
                        v.error(format!("{ctx}：b{bit} 的 name 为空"));
                    }
                }
            }
        }

        // 读写属性
        if p.access == hmi_domain::points::Access::Rw && !reg.area.writable() {
            v.error(format!("{ctx}：区域 {} 只读，不能声明为 rw", reg.area));
        }

        if p.scale == Some(0.0) {
            v.error(format!("{ctx}：scale 为 0（非法换算）"));
        }

        // 写值元数据：边界/步长/枚举都按工程量口径解释
        if p.is_bit_point() {
            if p.min.is_some() || p.max.is_some() || p.step.is_some() || !p.value_enum.is_empty() {
                v.error(format!("{ctx}：位区域点不支持 min/max/step/enum"));
            }
        } else {
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

        if p.data_type.word_count() > 1 && p.byte_order.is_none() {
            v.warn(format!(
                "{ctx}：多字类型未声明 byteOrder，将用包默认或 abcd（多字字节序是 485 头号坑，建议显式声明）"
            ));
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

    fn manifest() -> PackageManifest {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "id": "com.acme.pm3000",
            "name": "PM3000",
            "version": "1.0.0",
            "protocol": { "driver": "modbus-rtu" }
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
                { "id": "voltage", "register": "40101", "type": "u16", "scale": 0.1, "pollGroup": "fast" }
            ]
        }));
        let v = validate_package(&manifest(), &t);
        assert!(v.ok(), "{v:?}");
        assert!(v.warnings.is_empty());
    }

    #[test]
    fn catches_duplicates_and_bad_group() {
        let t = points_json(serde_json::json!({
            "pollGroups": [{ "name": "fast", "intervalMs": 0 }],
            "points": [
                { "id": "a", "register": "40101", "pollGroup": "fast" },
                { "id": "a", "register": "40102", "pollGroup": "nope" }
            ]
        }));
        let v = validate_package(&manifest(), &t);
        assert!(!v.ok());
        assert!(v.errors.iter().any(|e| e.contains("id 重复")));
        assert!(v.errors.iter().any(|e| e.contains("未定义的轮询组")));
        assert!(v.errors.iter().any(|e| e.contains("intervalMs 必须大于 0")));
    }

    #[test]
    fn catches_readonly_write() {
        let t = points_json(serde_json::json!({
            "points": [
                { "id": "ir", "register": "30100", "type": "u16", "access": "rw" }
            ]
        }));
        let v = validate_package(&manifest(), &t);
        assert!(v.errors.iter().any(|e| e.contains("只读")));
    }

    #[test]
    fn catches_bad_bits() {
        let t = points_json(serde_json::json!({
            "points": [
                { "id": "st", "register": "40300", "type": "u32",
                  "bits": { "20": { "name": "x" } } }
            ]
        }));
        let v = validate_package(&manifest(), &t);
        assert!(v.errors.iter().any(|e| e.contains("bits 只支持")));
    }

    #[test]
    fn warns_missing_byte_order_multiword() {
        let t = points_json(serde_json::json!({
            "points": [ { "id": "e", "register": "40200", "type": "u32" } ]
        }));
        let v = validate_package(&manifest(), &t);
        assert!(v.ok());
        assert!(v.warnings.iter().any(|w| w.contains("byteOrder")));
    }

    #[test]
    fn bad_manifest_id_rejected() {
        let mut m = manifest();
        m.id = "PM 3000".into();
        let v = validate_package(&m, &PointsTable::default());
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
        let v = validate_package(&m, &PointsTable::default());
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
        let v = validate_package(&m, &PointsTable::default());
        assert!(v.ok(), "{v:?}");
    }

    #[test]
    fn ui_relative_bindings_validated() {
        let t = points_json(serde_json::json!({
            "points": [
                { "id": "uab", "register": "40101", "type": "u16" },
                { "id": "status", "register": "40300", "type": "u16",
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
        let v = validate_package(&m, &t);
        assert!(!v.ok());
        assert!(v.errors.iter().any(|e| e.contains("不在点表")), "{v:?}");
        assert!(v.errors.iter().any(|e| e.contains("绝对 tag")), "{v:?}");
        assert!(v.errors.iter().any(|e| e.contains("没有位 3")), "{v:?}");
    }

    #[test]
    fn write_metadata_validated() {
        let t = points_json(serde_json::json!({
            "points": [
                { "id": "mode", "register": "40311", "type": "u16", "access": "rw",
                  "min": 0, "max": 2, "step": 1,
                  "enum": [ { "value": 0, "label": "自动" }, { "value": 1, "label": "手动" } ] },
                { "id": "bad", "register": "40312", "type": "f32", "access": "rw",
                  "min": 5, "max": 1, "step": 0,
                  "enum": [ { "value": 0, "label": "" } ] },
                { "id": "coil", "register": "00010", "min": 0 }
            ]
        }));
        let v = validate_package(&manifest(), &t);
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
        assert!(
            v.errors.iter().any(|e| e.contains("位区域点不支持")),
            "{v:?}"
        );
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
        let v = validate_package(&m, &PointsTable::default());
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
                { "id": "mode", "register": "40311", "type": "u16", "access": "rw",
                  "min": 0, "max": 2, "step": 1,
                  "enum": [ { "value": 0, "label": "自动" }, { "value": 2, "label": "停止" } ] }
            ]
        }));
        let v = validate_package(&manifest(), &t);
        assert!(v.ok(), "{v:?}");
    }
}
