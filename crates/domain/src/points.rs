//! 点表模型：设备包里"有哪些点、怎么换算"的领域表达。
//!
//! 点地址是 [`PointAddress`]：寄存器类协议用字符串（`"40101"` / `"HR:100"`），
//! 其他协议用对象形式承载驱动私有地址（如 BLE 的 service/characteristic）。
//! 驱动在 `validate_package` / `compile` 阶段解释并校验自己支持的地址形状。
//!
//! 这些类型是设备包 JSON 的唯一真相源；packaging 负责解析与通用校验，
//! 协议相关校验由驱动完成。

use crate::addr::RegAddr;
use crate::codec::{ByteOrder, DataType};
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

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

/// 点地址。
#[derive(Clone, Debug, PartialEq)]
pub enum PointAddress {
    /// 寄存器类协议地址（工程记法 `"40101"` 或显式 `"HR:100"`）
    Register(RegAddr),
    /// 驱动私有地址对象（BLE characteristic、JSON path 等）
    Structured(Json),
}

impl PointAddress {
    pub fn as_register(&self) -> Option<&RegAddr> {
        match self {
            PointAddress::Register(r) => Some(r),
            PointAddress::Structured(_) => None,
        }
    }
}

impl fmt::Display for PointAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PointAddress::Register(r) => f.write_str(&r.to_engineering()),
            PointAddress::Structured(v) => write!(f, "{v}"),
        }
    }
}

impl Serialize for PointAddress {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PointAddress::Register(r) => serializer.serialize_str(&r.to_engineering()),
            PointAddress::Structured(v) => v.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for PointAddress {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AddressVisitor;

        impl<'de> Visitor<'de> for AddressVisitor {
            type Value = PointAddress;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("寄存器地址字符串（如 40101 / HR:100）或协议地址对象")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                RegAddr::from_str(v)
                    .map(PointAddress::Register)
                    .map_err(E::custom)
            }

            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                self.visit_str(&v)
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
                let v = Json::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(PointAddress::Structured(v))
            }
        }

        deserializer.deserialize_any(AddressVisitor)
    }
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
    /// 协议地址
    pub address: PointAddress,
    /// 数据类型；位区域/非数值协议可忽略
    #[serde(rename = "type", alias = "dataType", default)]
    pub data_type: DataType,
    /// 字节序；None = 使用驱动默认
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
    /// 写值下限（工程量，含）；None = 不限制
    #[serde(default)]
    pub min: Option<f64>,
    /// 写值上限（工程量，含）；None = 不限制
    #[serde(default)]
    pub max: Option<f64>,
    /// 写值步长（工程量）；None = 不限制
    #[serde(default)]
    pub step: Option<f64>,
    /// 枚举写值表（仅整数类型；值按工程量口径）
    #[serde(rename = "enum", default, skip_serializing_if = "Vec::is_empty")]
    pub value_enum: Vec<EnumValueDef>,
}

/// 枚举点的一个可取项（写值下拉/按钮用）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumValueDef {
    /// 工程量取值（与 min/max 同口径，不乘 scale）
    pub value: i64,
    /// 显示文案（可中文）
    #[serde(default)]
    pub label: String,
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
    /// 是否为位地址（寄存器类协议：Coil / Discrete）。
    pub fn is_bit_point(&self) -> bool {
        matches!(&self.address, PointAddress::Register(r) if r.area.is_bit_area())
    }
}

/// 应用实例覆盖："points.<pointId>.scale" / ".offset" / ".pollGroup"。
///
/// 返回覆盖后的点表与警告；点表本身不被修改。
pub fn apply_overrides(
    table: &PointsTable,
    overrides: &serde_json::Map<String, Json>,
) -> (PointsTable, Vec<String>) {
    let mut out = table.clone();
    let mut warnings = Vec::new();
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
        let Some(p) = out.points.iter_mut().find(|p| p.id == pid) else {
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
    (out, warnings)
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
      "address": "40101",
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
      "address": "40300",
      "type": "u16",
      "pollGroup": "fast",
      "bits": { "0": { "name": "运行" }, "2": { "name": "报警" } }
    },
    {
      "id": "coil_out",
      "name": "输出线圈",
      "address": "00010"
    }
  ]
}
"#;

    #[test]
    fn parse_sample() {
        let t: PointsTable = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(t.points.len(), 3);
        let v = t.point("voltage_ab").unwrap();
        let r = v.address.as_register().unwrap();
        assert_eq!(r.offset, 100);
        assert_eq!(r.area, RegArea::Holding);
        assert_eq!(v.data_type, DataType::U16);
        assert_eq!(v.scale, Some(0.1));
        assert_eq!(v.access, Access::R);
        assert_eq!(v.poll_group.as_deref(), Some("fast"));
        assert!(t.poll_group("fast").is_some());

        let s = t.point("status").unwrap();
        assert_eq!(s.bits.len(), 2);
        assert_eq!(s.bits.get(&0).unwrap().name, "运行");

        let c = t.point("coil_out").unwrap();
        assert_eq!(c.data_type, DataType::U16); // 默认
        assert_eq!(PointsTable::effective_group(c), "default");
        assert!(c.is_bit_point());
    }

    #[test]
    fn bad_register_rejected_at_parse() {
        let bad = r#"{ "points": [ { "id": "x", "address": "99999" } ] }"#;
        let r: Result<PointsTable, _> = serde_json::from_str(bad);
        assert!(r.is_err());
    }

    #[test]
    fn structured_address_accepted() {
        let j = r#"
        { "points": [ {
            "id": "temp", "address": { "ble": { "service": "181A", "characteristic": "2A6E" } }
        } ] }"#;
        let t: PointsTable = serde_json::from_str(j).unwrap();
        match &t.point("temp").unwrap().address {
            PointAddress::Structured(v) => assert!(v.get("ble").is_some()),
            other => panic!("应为结构化地址: {other:?}"),
        }
    }

    #[test]
    fn parse_write_metadata() {
        let j = r#"
        {
          "points": [
            { "id": "mode", "address": "40311", "type": "u16", "access": "rw",
              "min": 0, "max": 2, "step": 1,
              "enum": [ { "value": 0, "label": "自动" }, { "value": 1, "label": "手动" } ] }
          ]
        }"#;
        let t: PointsTable = serde_json::from_str(j).unwrap();
        let p = t.point("mode").unwrap();
        assert_eq!(p.min, Some(0.0));
        assert_eq!(p.max, Some(2.0));
        assert_eq!(p.step, Some(1.0));
        assert_eq!(p.value_enum.len(), 2);
        assert_eq!(p.value_enum[1].value, 1);
        assert_eq!(p.value_enum[1].label, "手动");
    }

    #[test]
    fn overrides_apply_scale_offset_and_group() {
        let t: PointsTable = serde_json::from_str(SAMPLE).unwrap();
        let mut ov = serde_json::Map::new();
        ov.insert("points.voltage_ab.scale".into(), Json::from(0.01));
        ov.insert("points.voltage_ab.pollGroup".into(), Json::from("slow"));
        ov.insert("points.status.offset".into(), Json::from(5.0));
        ov.insert("points.missing.scale".into(), Json::from(1.0));
        let (out, warnings) = apply_overrides(&t, &ov);
        let v = out.point("voltage_ab").unwrap();
        assert_eq!(v.scale, Some(0.01));
        assert_eq!(v.poll_group.as_deref(), Some("slow"));
        assert_eq!(out.point("status").unwrap().offset, Some(5.0));
        assert_eq!(warnings.len(), 1);
    }
}
