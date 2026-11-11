//! 轮询编译器：设备点表 + 实例覆盖 → 按轮询组编译成"读块 + 解码映射"。
//!
//! 核心优化：同组内地址相邻/间隔 ≤ GAP 的寄存器合并成一个读块 —— 一个
//! 30 点的设备合并后往往只要 3~4 个事务，485 带宽预算的直接决定因素。

use hmi_domain::addr::RegAddr;
use hmi_domain::codec::{apply_scale, decode_registers, ByteOrder};
use hmi_domain::driver::{BlockData, BlockPayload, ReadBlock};
use hmi_domain::points::{PointDef, PointsTable, ProtocolDefaults};
use hmi_domain::value::{now_ms, Quality, TagId, TagSnapshot, Value};
use serde_json::Value as Json;
use std::time::Duration;
use thiserror::Error;

/// 合并阈值：相邻点间隔不超过 4 个寄存器就并进同一块
pub const MERGE_GAP: u32 = 4;
/// 单块最大字数（Modbus 协议限制）
pub const MAX_BLOCK_WORDS: u32 = 125;
/// 单块最大位数
pub const MAX_BLOCK_BITS: u32 = 2000;

/// 一个设备编译后的轮询计划。
#[derive(Clone, Debug)]
pub struct DevicePollPlan {
    pub device_id: String,
    pub groups: Vec<PollGroupPlan>,
    /// 编译期间收集的非致命问题（未知覆盖键等）
    pub warnings: Vec<String>,
}

/// 一个轮询组的计划：一次周期 = 读 blocks + 按 mappings 解码出 tags。
#[derive(Clone, Debug)]
pub struct PollGroupPlan {
    pub name: String,
    pub interval: Duration,
    pub blocks: Vec<ReadBlock>,
    pub mappings: Vec<Mapping>,
}

/// 解码映射：tag ← (块索引, 块内偏移, 点定义)。
#[derive(Clone, Debug)]
pub struct Mapping {
    pub tag: TagId,
    /// 点定义（byte_order 已解析为最终值）
    pub point: PointDef,
    pub block: usize,
    pub word_off: usize,
    pub kind: MapKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MapKind {
    /// 点本身的值
    Value,
    /// 状态字位展开（word_off 指向父点，bit 为位号）
    Bit(u8),
}

#[derive(Error, Debug, Clone)]
pub enum CompileError {
    #[error("点 `{0}` 所属轮询组未定义（校验应已拦截）")]
    UnknownGroup(String),
    #[error("编译器内部错误：{0}")]
    Internal(String),
}

/// 编译入口。
pub fn compile(
    device_id: &str,
    device_overrides: &serde_json::Map<String, Json>,
    table: &PointsTable,
    defaults: &ProtocolDefaults,
) -> Result<DevicePollPlan, CompileError> {
    let mut warnings = Vec::new();
    let points = apply_overrides(&table.points, device_overrides, &mut warnings);

    // 分组
    let mut group_names: Vec<String> = Vec::new();
    for p in &points {
        let g = p.poll_group.clone().unwrap_or_else(|| "default".into());
        if !group_names.contains(&g) {
            group_names.push(g);
        }
    }

    let mut groups = Vec::with_capacity(group_names.len());
    for gname in group_names {
        let interval_ms = table
            .poll_groups
            .iter()
            .find(|g| g.name == gname)
            .map(|g| g.interval_ms)
            .unwrap_or(1000);
        let mut gpoints: Vec<&PointDef> = points
            .iter()
            .filter(|p| p.poll_group.as_deref().unwrap_or("default") == gname)
            .collect();
        // 按区域与地址排序，保证合并的贪心正确性
        gpoints.sort_by_key(|p| (p.register.area as u8, p.register.offset));

        let blocks = merge_blocks(&gpoints);
        let mut mappings = Vec::new();
        for p in &gpoints {
            let wc = p.data_type.word_count() as u32;
            let (block_idx, word_off) = locate(&blocks, p.register, wc)
                .ok_or_else(|| CompileError::Internal(format!("点 {} 未落块", p.id)))?;
            // 点值映射
            mappings.push(Mapping {
                tag: TagId::join(device_id, &p.id),
                point: resolve_point(p, defaults),
                block: block_idx,
                word_off,
                kind: MapKind::Value,
            });
            // 位展开映射
            for (&bit, def) in &p.bits {
                let _ = def;
                mappings.push(Mapping {
                    tag: TagId::join_bit(device_id, &p.id, bit),
                    point: resolve_point(p, defaults),
                    block: block_idx,
                    word_off,
                    kind: MapKind::Bit(bit),
                });
            }
        }

        groups.push(PollGroupPlan {
            name: gname,
            interval: Duration::from_millis(interval_ms),
            blocks,
            mappings,
        });
    }

    Ok(DevicePollPlan {
        device_id: device_id.to_string(),
        groups,
        warnings,
    })
}

fn resolve_point(p: &PointDef, defaults: &ProtocolDefaults) -> PointDef {
    let mut c = p.clone();
    if c.byte_order.is_none() {
        c.byte_order = Some(defaults.byte_order.unwrap_or_default());
    }
    c
}

/// 应用实例覆盖："points.<pointId>.scale" / ".offset" / ".pollGroup"。
fn apply_overrides(
    points: &[PointDef],
    overrides: &serde_json::Map<String, Json>,
    warnings: &mut Vec<String>,
) -> Vec<PointDef> {
    let mut out = points.to_vec();
    for (key, val) in overrides {
        let Some(rest) = key.strip_prefix("points.") else {
            warnings.push(format!(
                "忽略未知覆盖键 `{key}`（支持 points.<id>.scale/.offset/.pollGroup）"
            ));
            continue;
        };
        let Some((pid, field)) = rest.rsplit_once('.') else {
            warnings.push(format!("忽略格式错误的覆盖键 `{key}`"));
            continue;
        };
        let Some(p) = out.iter_mut().find(|p| p.id == pid) else {
            warnings.push(format!("覆盖键 `{key}` 指向不存在的点"));
            continue;
        };
        match field {
            "scale" => match val.as_f64() {
                Some(v) => p.scale = Some(v),
                None => warnings.push(format!("覆盖键 `{key}` 的值不是数值")),
            },
            "offset" => match val.as_f64() {
                Some(v) => p.offset = Some(v),
                None => warnings.push(format!("覆盖键 `{key}` 的值不是数值")),
            },
            "pollGroup" => match val.as_str() {
                Some(v) => p.poll_group = Some(v.to_string()),
                None => warnings.push(format!("覆盖键 `{key}` 的值不是字符串")),
            },
            other => warnings.push(format!("忽略不支持的覆盖字段 `{other}`")),
        }
    }
    out
}

/// 贪心合并：同区域、间隔 ≤ MERGE_GAP、块长不超限 → 并块。
fn merge_blocks(points: &[&PointDef]) -> Vec<ReadBlock> {
    let mut blocks: Vec<ReadBlock> = Vec::new();
    for p in points {
        let wc = p.data_type.word_count().max(1) as u32;
        let max = if p.register.area.is_bit_area() {
            MAX_BLOCK_BITS
        } else {
            MAX_BLOCK_WORDS
        };
        let end = p.register.offset as u32 + wc; // 开区间末尾

        let extendable = blocks
            .last()
            .map(|b| {
                b.area == p.register.area
                    && p.register.offset as u32 >= b.start as u32 + b.count as u32 // 不回退
                    && p.register.offset as u32 - (b.start as u32 + b.count as u32) <= MERGE_GAP
                    && end - b.start as u32 <= max
            })
            .unwrap_or(false);

        if extendable {
            let b = blocks.last_mut().unwrap();
            b.count = (end - b.start as u32) as u16;
        } else {
            blocks.push(ReadBlock {
                area: p.register.area,
                start: p.register.offset,
                count: wc as u16,
            });
        }
    }
    blocks
}

fn locate(blocks: &[ReadBlock], reg: RegAddr, wc: u32) -> Option<(usize, usize)> {
    blocks
        .iter()
        .position(|b| {
            b.area == reg.area
                && reg.offset >= b.start
                && reg.offset as u32 + wc <= b.start as u32 + b.count as u32
        })
        .map(|i| (i, (reg.offset - blocks[i].start) as usize))
}

impl PollGroupPlan {
    /// 把读块结果解码成 tag 快照批。
    pub fn decode(&self, device_id: &str, data: &[BlockData]) -> Vec<(TagId, TagSnapshot)> {
        let ts = now_ms();
        let mut out = Vec::with_capacity(self.mappings.len());
        for m in &self.mappings {
            let Some(bd) = data.get(m.block) else {
                continue;
            };
            let snap = match (&bd.payload, m.kind, m.point.is_bit_point()) {
                (BlockPayload::Bits(bits), MapKind::Value, true) => {
                    bits.get(m.word_off).map(|&b| TagSnapshot {
                        value: Value::Bool(b),
                        quality: Quality::Good,
                        ts_ms: ts,
                    })
                }
                (BlockPayload::Words(words), MapKind::Value, false) => {
                    let wc = m.point.data_type.word_count();
                    if m.word_off + wc > words.len() {
                        continue;
                    }
                    let slice = &words[m.word_off..m.word_off + wc];
                    let bo: ByteOrder = m.point.byte_order.unwrap_or_default();
                    match decode_registers(m.point.data_type, bo, slice) {
                        Ok(num) => Some(TagSnapshot {
                            value: apply_scale(num, m.point.scale, m.point.offset),
                            quality: Quality::Good,
                            ts_ms: ts,
                        }),
                        Err(e) => {
                            tracing::warn!(tag = %m.tag, error = %e, "解码失败，置 Bad");
                            Some(TagSnapshot {
                                value: Value::F64(0.0),
                                quality: Quality::Bad,
                                ts_ms: ts,
                            })
                        }
                    }
                }
                (BlockPayload::Words(words), MapKind::Bit(bit), false) => {
                    words.get(m.word_off).map(|&w| TagSnapshot {
                        value: Value::Bool(hmi_domain::codec::extract_bit(w, bit)),
                        quality: Quality::Good,
                        ts_ms: ts,
                    })
                }
                _ => {
                    tracing::warn!(tag = %m.tag, device = device_id, "映射与载荷类型不匹配");
                    None
                }
            };
            if let Some(s) = snap {
                out.push((m.tag.clone(), s));
            }
        }
        out
    }

    /// 该组产出的全部 tag（CommLost 标记用）。
    pub fn all_tags(&self) -> Vec<TagId> {
        self.mappings.iter().map(|m| m.tag.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmi_domain::addr::RegArea;
    use hmi_domain::points::{Access, BitDef, PollGroupDef};
    use std::collections::BTreeMap;

    fn point(id: &str, off: u16, dt: hmi_domain::codec::DataType) -> PointDef {
        PointDef {
            id: id.into(),
            name: String::new(),
            register: RegAddr {
                area: RegArea::Holding,
                offset: off,
            },
            data_type: dt,
            byte_order: None,
            scale: None,
            offset: None,
            unit: String::new(),
            access: Access::R,
            poll_group: None,
            bits: BTreeMap::new(),
        }
    }

    fn table(points: Vec<PointDef>) -> PointsTable {
        PointsTable {
            poll_groups: vec![PollGroupDef {
                name: "fast".into(),
                interval_ms: 1000,
            }],
            points,
        }
    }

    #[test]
    fn merge_contiguous_and_near() {
        // 0,1,2 连续 + 8（间隔 5 > GAP? 8 - 3 = 5 > 4 → 新块）；10 与 8 间隔 0 → 并
        let t = table(vec![
            point("a", 0, hmi_domain::codec::DataType::U16),
            point("b", 1, hmi_domain::codec::DataType::U16),
            point("c", 2, hmi_domain::codec::DataType::U16),
            point("d", 8, hmi_domain::codec::DataType::U16),
            point("e", 10, hmi_domain::codec::DataType::U16),
        ]);
        let plan = compile("dev1", &Default::default(), &t, &Default::default()).unwrap();
        let g = &plan.groups[0];
        assert_eq!(g.blocks.len(), 2);
        assert_eq!((g.blocks[0].start, g.blocks[0].count), (0, 3));
        assert_eq!((g.blocks[1].start, g.blocks[1].count), (8, 3));
    }

    #[test]
    fn merge_gap_boundary() {
        // a 占 0-1；b 在 5：5 - 2 = 3 ≤ GAP → 并（块覆盖 0..5，count=6）；
        // c 在 11：11 - 6 = 5 > GAP → 新块
        let t = table(vec![
            point("a", 0, hmi_domain::codec::DataType::U32), // 占 0-1
            point("b", 5, hmi_domain::codec::DataType::U16), // 5 - 2 = 3 ≤ 4 → 并
            point("c", 11, hmi_domain::codec::DataType::U16), // 11 - 6 = 5 > 4 → 新块
        ]);
        let plan = compile("dev1", &Default::default(), &t, &Default::default()).unwrap();
        let g = &plan.groups[0];
        assert_eq!(g.blocks.len(), 2);
        assert_eq!(g.blocks[0].count, 6); // 覆盖 0..5
        assert_eq!(g.blocks[1].start, 11);
    }

    #[test]
    fn groups_split_by_interval() {
        let mut t = table(vec![]);
        t.poll_groups.push(PollGroupDef {
            name: "slow".into(),
            interval_ms: 60000,
        });
        let mut fast = point("f", 0, hmi_domain::codec::DataType::U16);
        fast.poll_group = Some("fast".into());
        let mut slow = point("s", 100, hmi_domain::codec::DataType::U16);
        slow.poll_group = Some("slow".into());
        t.points = vec![fast, slow];

        let plan = compile("dev1", &Default::default(), &t, &Default::default()).unwrap();
        assert_eq!(plan.groups.len(), 2);
        let slow_g = plan.groups.iter().find(|g| g.name == "slow").unwrap();
        assert_eq!(slow_g.interval, Duration::from_millis(60000));
        // 点只落进自己的组
        assert_eq!(slow_g.mappings.len(), 1);
    }

    #[test]
    fn overrides_applied() {
        let t = table(vec![point("v", 0, hmi_domain::codec::DataType::U16)]);
        let mut ov = serde_json::Map::new();
        ov.insert("points.v.scale".into(), serde_json::json!(0.01));
        let plan = compile("dev1", &ov, &t, &Default::default()).unwrap();
        let m = &plan.groups[0].mappings[0];
        assert_eq!(m.point.scale, Some(0.01));

        // 未知键 → warning
        let mut bad = serde_json::Map::new();
        bad.insert("points.v.color".into(), serde_json::json!("red"));
        let plan = compile("dev1", &bad, &t, &Default::default()).unwrap();
        assert!(!plan.warnings.is_empty());
    }

    #[test]
    fn byte_order_resolution_chain() {
        let t = table(vec![point("e", 0, hmi_domain::codec::DataType::U32)]);
        let defaults = ProtocolDefaults {
            byte_order: Some(ByteOrder::Cdab),
            resp_timeout_ms: None,
        };
        let plan = compile("dev1", &Default::default(), &t, &defaults).unwrap();
        assert_eq!(
            plan.groups[0].mappings[0].point.byte_order,
            Some(ByteOrder::Cdab)
        );
    }

    #[test]
    fn decode_produces_scaled_values_and_bits() {
        let mut p = point("v", 0, hmi_domain::codec::DataType::U16);
        p.scale = Some(0.1);
        let mut st = point("st", 1, hmi_domain::codec::DataType::U16);
        st.bits.insert(
            2,
            BitDef {
                name: "报警".into(),
                on: None,
                off: None,
            },
        );
        let t = table(vec![p, st]);
        let plan = compile("dev1", &Default::default(), &t, &Default::default()).unwrap();
        let g = &plan.groups[0];

        let data = vec![BlockData {
            area: RegArea::Holding,
            start: 0,
            payload: BlockPayload::Words(vec![2200, 0b0000_0100]),
        }];
        let batch = g.decode("dev1", &data);
        let map: std::collections::HashMap<_, _> = batch.into_iter().map(|(k, v)| (k, v)).collect();

        let v = map.get(&TagId::new("dev1.v")).unwrap();
        assert_eq!(v.value, Value::F64(220.0));
        assert_eq!(v.quality, Quality::Good);

        let b = map.get(&TagId::new("dev1.st.b2")).unwrap();
        assert_eq!(b.value, Value::Bool(true));
    }

    #[test]
    fn coil_points_map_to_bits() {
        let mut c = point("coil", 0, hmi_domain::codec::DataType::U16);
        c.register = RegAddr {
            area: RegArea::Coil,
            offset: 3,
        };
        let t = table(vec![c]);
        let plan = compile("dev1", &Default::default(), &t, &Default::default()).unwrap();
        let g = &plan.groups[0];
        assert_eq!(g.blocks[0].area, RegArea::Coil);
        assert_eq!(g.blocks[0].count, 1);

        let data = vec![BlockData {
            area: RegArea::Coil,
            start: 3,
            payload: BlockPayload::Bits(vec![true]),
        }];
        let batch = g.decode("dev1", &data);
        assert_eq!(batch[0].0, TagId::new("dev1.coil"));
        assert_eq!(batch[0].1.value, Value::Bool(true));
    }
}
