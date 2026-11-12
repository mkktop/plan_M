//! Modbus 点表编译：点表 + 实例覆盖 → 读块合并 + 解码映射。
//!
//! 这是 Modbus 专属逻辑（寄存器区域、125 字/2000 位上限、位展开），
//! 因此放在驱动 crate，而不是通用 gateway。

use hmi_domain::addr::{RegAddr, RegArea};
use hmi_domain::codec::{apply_scale, decode_registers, extract_bit, ByteOrder};
use hmi_domain::driver::{CompileInput, DevicePlan, DriverError, PlanPayload, PollGroup, Sample};
use hmi_domain::link::TransportKind;
use hmi_domain::points::{apply_overrides, PointAddress, PointDef};
use hmi_domain::value::{Quality, TagId, Value};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 合并阈值：相邻点间隔不超过 4 个寄存器就并进同一块
pub const MERGE_GAP: u32 = 4;
/// 单块最大字数（Modbus 协议限制）
pub const MAX_BLOCK_WORDS: u32 = 125;
/// 单块最大位数
pub const MAX_BLOCK_BITS: u32 = 2000;

/// 设备包 `protocol.params` 的 Modbus 形状。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModbusParams {
    /// 帧封装：auto 按传输类型选择，rtu/tcp 显式指定（支持 TCP 转 485 网关）
    #[serde(default)]
    pub framing: Framing,
    /// 多字类型缺省字节序
    #[serde(default)]
    pub byte_order: Option<ByteOrder>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Framing {
    #[default]
    Auto,
    Rtu,
    Tcp,
}

/// 编译期无法确定传输类型时保留 Auto，执行时再解析。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireFraming {
    Rtu,
    Tcp,
}

impl Framing {
    pub fn resolve(self, kind: TransportKind) -> WireFraming {
        match self {
            Framing::Rtu => WireFraming::Rtu,
            Framing::Tcp => WireFraming::Tcp,
            Framing::Auto => {
                if kind == TransportKind::Tcp {
                    WireFraming::Tcp
                } else {
                    WireFraming::Rtu
                }
            }
        }
    }
}

/// 一块连续读取请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadBlock {
    pub area: RegArea,
    pub start: u16,
    pub count: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlockPayload {
    Words(Vec<u16>),
    Bits(Vec<bool>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockData {
    pub area: RegArea,
    pub start: u16,
    pub payload: BlockPayload,
}

/// 协议私有执行计划（存进 `PollGroup::payload`）。
#[derive(Clone, Debug)]
pub struct ModbusGroupPlan {
    pub framing: Framing,
    pub blocks: Vec<ReadBlock>,
    pub mappings: Vec<Mapping>,
}

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
    Value,
    Bit(u8),
}

/// 编译入口。
pub fn compile(input: &CompileInput<'_>) -> Result<DevicePlan, DriverError> {
    let params: ModbusParams = input.protocol.params_as().map_err(DriverError::Compile)?;
    let (table, mut warnings) = apply_overrides(input.points, input.overrides);

    for p in &table.points {
        if !matches!(p.address, PointAddress::Register(_)) {
            return Err(DriverError::Compile(format!(
                "点 `{}` 使用了非寄存器地址，modbus 驱动不支持",
                p.id
            )));
        }
    }

    // 分组（保持点表出现顺序）
    let mut group_names: Vec<String> = Vec::new();
    for p in &table.points {
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
        let mut gpoints: Vec<&PointDef> = table
            .points
            .iter()
            .filter(|p| p.poll_group.as_deref().unwrap_or("default") == gname)
            .collect();
        // 按区域与地址排序，保证合并的贪心正确性
        gpoints.sort_by_key(|p| {
            let r = register_of(p).expect("compile 入口已校验");
            (r.area as u8, r.offset)
        });

        let blocks = merge_blocks(&gpoints);
        let mut mappings = Vec::new();
        for p in &gpoints {
            let reg = register_of(p).expect("compile 入口已校验");
            let wc = p.data_type.word_count().max(1) as u32;
            let (block_idx, word_off) = locate(&blocks, reg, wc)
                .ok_or_else(|| DriverError::Compile(format!("点 `{}` 未落入任何读块", p.id)))?;
            let resolved = resolve_point(p, &params);
            mappings.push(Mapping {
                tag: TagId::join(input.device_id, &p.id),
                point: resolved.clone(),
                block: block_idx,
                word_off,
                kind: MapKind::Value,
            });
            for &bit in p.bits.keys() {
                mappings.push(Mapping {
                    tag: TagId::join_bit(input.device_id, &p.id, bit),
                    point: resolved.clone(),
                    block: block_idx,
                    word_off,
                    kind: MapKind::Bit(bit),
                });
            }
        }

        let tags = mappings.iter().map(|m| m.tag.clone()).collect();
        let payload = ModbusGroupPlan {
            framing: params.framing,
            blocks,
            mappings,
        };
        groups.push(PollGroup {
            name: gname,
            interval: Duration::from_millis(interval_ms),
            tags,
            payload: PlanPayload::new(payload),
        });
    }

    // 去重 warnings（overrides 已给出）
    warnings.shrink_to_fit();
    Ok(DevicePlan { groups, warnings })
}

fn register_of(p: &PointDef) -> Option<RegAddr> {
    match &p.address {
        PointAddress::Register(r) => Some(*r),
        PointAddress::Structured(_) => None,
    }
}

fn resolve_point(p: &PointDef, params: &ModbusParams) -> PointDef {
    let mut c = p.clone();
    if c.byte_order.is_none() {
        c.byte_order = Some(params.byte_order.unwrap_or_default());
    }
    c
}

/// 贪心合并：同区域、间隔 ≤ MERGE_GAP、块长不超限 → 并块。
fn merge_blocks(points: &[&PointDef]) -> Vec<ReadBlock> {
    let mut blocks: Vec<ReadBlock> = Vec::new();
    for p in points {
        let reg = register_of(p).expect("compile 入口已校验");
        let wc = p.data_type.word_count().max(1) as u32;
        let max = if reg.area.is_bit_area() {
            MAX_BLOCK_BITS
        } else {
            MAX_BLOCK_WORDS
        };
        let end = reg.offset as u32 + wc; // 开区间末尾

        let extendable = blocks
            .last()
            .map(|b| {
                b.area == reg.area
                    && reg.offset as u32 >= b.start as u32 + b.count as u32 // 不回退
                    && reg.offset as u32 - (b.start as u32 + b.count as u32) <= MERGE_GAP
                    && end - b.start as u32 <= max
            })
            .unwrap_or(false);

        if extendable {
            let b = blocks.last_mut().unwrap();
            b.count = (end - b.start as u32) as u16;
        } else {
            blocks.push(ReadBlock {
                area: reg.area,
                start: reg.offset,
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

/// 把读块结果解码成 tag 样本批。
pub fn decode(plan: &ModbusGroupPlan, data: &[BlockData]) -> Vec<Sample> {
    let mut out = Vec::with_capacity(plan.mappings.len());
    for m in &plan.mappings {
        let Some(bd) = data.get(m.block) else {
            continue;
        };
        let sample = match (&bd.payload, m.kind, m.point.is_bit_point()) {
            (BlockPayload::Bits(bits), MapKind::Value, true) => bits
                .get(m.word_off)
                .map(|&b| Sample::good(m.tag.clone(), Value::Bool(b))),
            (BlockPayload::Words(words), MapKind::Value, false) => {
                let wc = m.point.data_type.word_count();
                if m.word_off + wc > words.len() {
                    continue;
                }
                let slice = &words[m.word_off..m.word_off + wc];
                let bo = m.point.byte_order.unwrap_or_default();
                match decode_registers(m.point.data_type, bo, slice) {
                    Ok(num) => Some(Sample::good(
                        m.tag.clone(),
                        apply_scale(num, m.point.scale, m.point.offset),
                    )),
                    Err(e) => {
                        tracing::warn!(tag = %m.tag, error = %e, "解码失败，置 Bad");
                        Some(Sample {
                            tag: m.tag.clone(),
                            value: Value::F64(0.0),
                            quality: Quality::Bad,
                        })
                    }
                }
            }
            (BlockPayload::Words(words), MapKind::Bit(bit), false) => words
                .get(m.word_off)
                .map(|&w| Sample::good(m.tag.clone(), Value::Bool(extract_bit(w, bit)))),
            _ => {
                tracing::warn!(tag = %m.tag, "映射与载荷类型不匹配");
                None
            }
        };
        if let Some(s) = sample {
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmi_domain::driver::CompileInput;
    use hmi_domain::points::{Access, BitDef, PointAddress, PointsTable, PollGroupDef};
    use hmi_domain::protocol::ProtocolDecl;
    use std::collections::BTreeMap;

    fn point(id: &str, off: u16, dt: hmi_domain::codec::DataType) -> PointDef {
        PointDef {
            id: id.into(),
            name: String::new(),
            address: PointAddress::Register(RegAddr {
                area: RegArea::Holding,
                offset: off,
            }),
            data_type: dt,
            byte_order: None,
            scale: None,
            offset: None,
            unit: String::new(),
            access: Access::R,
            poll_group: None,
            bits: BTreeMap::new(),
            min: None,
            max: None,
            step: None,
            value_enum: Vec::new(),
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

    fn decl(params: serde_json::Value) -> ProtocolDecl {
        ProtocolDecl {
            driver: "modbus".into(),
            transports: Vec::new(),
            params,
        }
    }

    fn run(
        t: &PointsTable,
        ov: serde_json::Map<String, serde_json::Value>,
        params: serde_json::Value,
    ) -> DevicePlan {
        let d = decl(params);
        compile(&CompileInput {
            device_id: "dev1",
            overrides: &ov,
            points: t,
            protocol: &d,
        })
        .unwrap()
    }

    fn group_plan(g: &PollGroup) -> &ModbusGroupPlan {
        g.payload.downcast_ref::<ModbusGroupPlan>().unwrap()
    }

    #[test]
    fn merge_contiguous_and_near() {
        let t = table(vec![
            point("a", 0, hmi_domain::codec::DataType::U16),
            point("b", 1, hmi_domain::codec::DataType::U16),
            point("c", 2, hmi_domain::codec::DataType::U16),
            point("d", 8, hmi_domain::codec::DataType::U16),
            point("e", 10, hmi_domain::codec::DataType::U16),
        ]);
        let plan = run(&t, Default::default(), serde_json::json!({}));
        let g = group_plan(&plan.groups[0]);
        assert_eq!(g.blocks.len(), 2);
        assert_eq!((g.blocks[0].start, g.blocks[0].count), (0, 3));
        assert_eq!((g.blocks[1].start, g.blocks[1].count), (8, 3));
    }

    #[test]
    fn merge_gap_boundary() {
        let t = table(vec![
            point("a", 0, hmi_domain::codec::DataType::U32),
            point("b", 5, hmi_domain::codec::DataType::U16),
            point("c", 11, hmi_domain::codec::DataType::U16),
        ]);
        let plan = run(&t, Default::default(), serde_json::json!({}));
        let g = group_plan(&plan.groups[0]);
        assert_eq!(g.blocks.len(), 2);
        assert_eq!(g.blocks[0].count, 6);
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

        let plan = run(&t, Default::default(), serde_json::json!({}));
        assert_eq!(plan.groups.len(), 2);
        let slow_g = plan.groups.iter().find(|g| g.name == "slow").unwrap();
        assert_eq!(slow_g.interval, Duration::from_millis(60000));
        assert_eq!(group_plan(slow_g).mappings.len(), 1);
    }

    #[test]
    fn overrides_applied() {
        let t = table(vec![point("v", 0, hmi_domain::codec::DataType::U16)]);
        let mut ov = serde_json::Map::new();
        ov.insert("points.v.scale".into(), serde_json::json!(0.01));
        let plan = run(&t, ov, serde_json::json!({}));
        let g = group_plan(&plan.groups[0]);
        assert_eq!(g.mappings[0].point.scale, Some(0.01));

        let mut bad = serde_json::Map::new();
        bad.insert("points.v.color".into(), serde_json::json!("red"));
        let plan = run(&t, bad, serde_json::json!({}));
        assert!(!plan.warnings.is_empty());
    }

    #[test]
    fn byte_order_resolution_chain() {
        let t = table(vec![point("e", 0, hmi_domain::codec::DataType::U32)]);
        let plan = run(
            &t,
            Default::default(),
            serde_json::json!({"byteOrder":"cdab"}),
        );
        let g = group_plan(&plan.groups[0]);
        assert_eq!(g.mappings[0].point.byte_order, Some(ByteOrder::Cdab));
    }

    #[test]
    fn framing_auto_and_explicit() {
        let t = table(vec![point("v", 0, hmi_domain::codec::DataType::U16)]);
        let plan = run(&t, Default::default(), serde_json::json!({}));
        let g = group_plan(&plan.groups[0]);
        assert_eq!(g.framing.resolve(TransportKind::Serial), WireFraming::Rtu);
        assert_eq!(g.framing.resolve(TransportKind::Tcp), WireFraming::Tcp);

        let plan = run(&t, Default::default(), serde_json::json!({"framing":"rtu"}));
        let g = group_plan(&plan.groups[0]);
        assert_eq!(g.framing.resolve(TransportKind::Tcp), WireFraming::Rtu);
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
        let plan = run(&t, Default::default(), serde_json::json!({}));
        let g = group_plan(&plan.groups[0]);

        let data = vec![BlockData {
            area: RegArea::Holding,
            start: 0,
            payload: BlockPayload::Words(vec![2200, 0b0000_0100]),
        }];
        let batch = decode(g, &data);
        let map: std::collections::HashMap<_, _> =
            batch.into_iter().map(|s| (s.tag, s.value)).collect();

        assert_eq!(map.get(&TagId::new("dev1.v")), Some(&Value::F64(220.0)));
        assert_eq!(map.get(&TagId::new("dev1.st.b2")), Some(&Value::Bool(true)));
    }

    #[test]
    fn coil_points_map_to_bits() {
        let mut c = point("coil", 0, hmi_domain::codec::DataType::U16);
        c.address = PointAddress::Register(RegAddr {
            area: RegArea::Coil,
            offset: 3,
        });
        let t = table(vec![c]);
        let plan = run(&t, Default::default(), serde_json::json!({}));
        let g = group_plan(&plan.groups[0]);
        assert_eq!(g.blocks[0].area, RegArea::Coil);
        assert_eq!(g.blocks[0].count, 1);

        let data = vec![BlockData {
            area: RegArea::Coil,
            start: 3,
            payload: BlockPayload::Bits(vec![true]),
        }];
        let batch = decode(g, &data);
        assert_eq!(batch[0].tag, TagId::new("dev1.coil"));
        assert_eq!(batch[0].value, Value::Bool(true));
    }

    #[test]
    fn structured_address_is_rejected() {
        let mut p = point("x", 0, hmi_domain::codec::DataType::U16);
        p.address = PointAddress::Structured(serde_json::json!({"ble": {}}));
        let t = table(vec![p]);
        let d = decl(serde_json::json!({}));
        let r = compile(&CompileInput {
            device_id: "dev1",
            overrides: &Default::default(),
            points: &t,
            protocol: &d,
        });
        assert!(matches!(r, Err(DriverError::Compile(_))));
    }
}
