//! 点表模型：设备包里"有哪些点、怎么读、怎么换算"的领域表达。
//!
//! 这些类型是设备包 JSON 的唯一真相源（packaging crate 负责解析与校验，
//! gateway crate 负责编译成轮询计划）。

use crate::addr::RegAddr;
use crate::codec::{ByteOrder, DataType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 整个设备包的点表文件（points.json）。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointsTable {
    #[serde(default)]
    pub poll_groups: Vec<PollGroupDef>,
    #[serde(default)]
    pub points: Vec<PointDef>,
}

/// 轮询组定义。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PollGroupDef {
    pub name: String,
    #[serde(default = "default_interval_ms")]
    pub interval_ms: u64,
}

fn default_interval_ms() -> u64 {
    1000
}

/// 单个数据点定义。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PointDef {
    /// 点内唯一 id（ascii 标识符），tag 全名 = `<设备id>.<point_id>`
    pub id: String,
    /// 显示名（可中文）
    #[serde(default)]
    pub name: String,
    /// 寄存器地址，工程记法（"40101"）或显式记法（"HR:100"）
    pub register: RegAddr,
    /// 数据类型；位区域（Coil/Discrete）忽略此字段
    #[serde(rename = "type", alias = "dataType", default)]
    pub data_type: DataType,
    /// 字节序；None = 继承 manifest 的 protocol.defaults
    #[serde(default)]
    pub byte_order: Option<ByteOrder>,
    /// 工程量换算 value * scale + offset；None = 1
    #[serde(default)]
    pub scale: Option<f64>,
    /// None = 0
    #[serde(default)]
    pub offset: Option<f64>,
    /// 工程单位（V / kWh / ...）
    #[serde(default)]
    pub unit: String,
    /// 读写属性
    #[serde(default)]
    pub access: Access,
    /// 所属轮询组；None = 默认组（1000ms）
    #[serde(default)]
    pub poll_group: Option<String>,
    /// 状态字位展开：位号 → 定义。生成 `<point>.b<N>` 子 tag。
    #[serde(default)]
    pub bits: BTreeMap<u8, BitDef>,
}

/// 读写属性。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    /// 只读
    #[default]
    R,
    /// 读写
    Rw,
}

/// 状态字中的一位。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitDef {
    /// 位显示名
    pub name: String,
    /// 置位时的文案（默认 "1"）
    #[serde(default)]
    pub on: Option<String>,
    /// 复位时的文案（默认 "0"）
    #[serde(default)]
    pub off: Option<String>,
}

/// manifest 中 protocol.defaults 段：点表字段的包级默认值。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolDefaults {
    #[serde(default)]
    pub byte_order: Option<ByteOrder>,
    #[serde(default)]
    pub resp_timeout_ms: Option<u32>,
}

impl PointsTable {
    /// 查找轮询组定义。
    pub fn poll_group(&self, name: &str) -> Option<&PollGroupDef> {
        self.poll_groups.iter().find(|g| g.name == name)
    }

    /// 点的有效轮询组名（None → "default"）。
    pub fn effective_group(point: &PointDef) -> &str {
        point.poll_group.as_deref().unwrap_or("default")
    }

    pub fn point(&self, id: &str) -> Option<&PointDef> {
        self.points.iter().find(|p| p.id == id)
    }
}

impl PointDef {
    pub fn is_bit_point(&self) -> bool {
        self.register.area.is_bit_area()
    }

    /// 解析后的字节序：点级 → 包默认 → Abcd。
    pub fn resolved_byte_order(&self, defaults: &ProtocolDefaults) -> ByteOrder {
        self.byte_order.or(defaults.byte_order).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr::RegArea;

    const SAMPLE: &str = r#"
{
  "pollGroups": [
    { "name": "fast", "intervalMs": 1000 },
    { "name": "slow", "intervalMs": 60000 }
  ],
  "points": [
    {
      "id": "voltage_ab",
      "name": "AB线电压",
      "register": "40101",
      "type": "u16",
      "byteOrder": "abcd",
      "scale": 0.1,
      "unit": "V",
      "access": "r",
      "pollGroup": "fast"
    },
    {
      "id": "status",
      "name": "状态字",
      "register": "40300",
      "type": "u16",
      "pollGroup": "fast",
      "bits": { "0": { "name": "运行" }, "2": { "name": "报警" } }
    },
    {
      "id": "coil_out",
      "name": "输出线圈",
      "register": "00010"
    }
  ]
}
"#;

    #[test]
    fn parse_sample() {
        let t: PointsTable = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(t.points.len(), 3);
        let v = t.point("voltage_ab").unwrap();
        assert_eq!(v.register.offset, 100);
        assert_eq!(v.register.area, RegArea::Holding);
        assert_eq!(v.data_type, DataType::U16);
        assert_eq!(v.scale, Some(0.1));
        assert_eq!(v.access, Access::R);
        assert_eq!(v.poll_group.as_deref(), Some("fast"));
        assert!(t.poll_group("fast").is_some());

        let s = t.point("status").unwrap();
        assert_eq!(s.bits.len(), 2);
        assert_eq!(s.bits.get(&0).unwrap().name, "运行");

        // 无 pollGroup / 无 type / 位区域
        let c = t.point("coil_out").unwrap();
        assert_eq!(c.data_type, DataType::U16); // 默认
        assert_eq!(PointsTable::effective_group(c), "default");
        assert!(c.is_bit_point());
    }

    #[test]
    fn bad_register_rejected_at_parse() {
        let bad = r#"{ "points": [ { "id": "x", "register": "99999" } ] }"#;
        let r: Result<PointsTable, _> = serde_json::from_str(bad);
        assert!(r.is_err());
    }

    #[test]
    fn resolved_byte_order_chain() {
        let mut p = PointDef {
            id: "x".into(),
            name: String::new(),
            register: "HR:1".parse().unwrap(),
            data_type: DataType::U32,
            byte_order: None,
            scale: None,
            offset: None,
            unit: String::new(),
            access: Access::R,
            poll_group: None,
            bits: BTreeMap::new(),
        };
        let defaults = ProtocolDefaults {
            byte_order: Some(ByteOrder::Cdab),
            resp_timeout_ms: None,
        };
        assert_eq!(p.resolved_byte_order(&defaults), ByteOrder::Cdab);
        p.byte_order = Some(ByteOrder::Badc);
        assert_eq!(p.resolved_byte_order(&defaults), ByteOrder::Badc);
        p.byte_order = None; // 回到包默认链
        assert_eq!(
            p.resolved_byte_order(&ProtocolDefaults::default()),
            ByteOrder::Abcd
        );
    }
}
