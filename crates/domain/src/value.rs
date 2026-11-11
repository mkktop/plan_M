//! Tag 值、质量戳与实时库的基本单位。

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// 一个 tag 的运行时值。窄类型，刻意不包容万象：
/// 数值走 F64/I64，开关量走 Bool，字符串留给将来的字符串型寄存器。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Value {
    F64(f64),
    I64(i64),
    Bool(bool),
    Str(String),
}

impl Value {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::F64(v) => Some(*v),
            Value::I64(v) => Some(*v as f64),
            Value::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
            Value::Str(_) => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::I64(v) => Some(*v),
            Value::F64(v) => Some(*v as i64),
            Value::Bool(v) => Some(if *v { 1 } else { 0 }),
            Value::Str(_) => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::F64(_) => "f64",
            Value::I64(_) => "i64",
            Value::Bool(_) => "bool",
            Value::Str(_) => "string",
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::F64(v) => write!(f, "{v}"),
            Value::I64(v) => write!(f, "{v}"),
            Value::Bool(v) => write!(f, "{v}"),
            Value::Str(v) => write!(f, "{v}"),
        }
    }
}

/// 质量戳。从第一天就跟随每个值，UI 靠它区分"0"和"通信断了不知道"。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// 值可信
    #[default]
    Good,
    /// 值拿到了但内容可疑（CRC 通过但语义异常、解码越界等）
    Bad,
    /// 值来自推算/陈旧数据
    Uncertain,
    /// 通信中断，值为最后一次好值或占位
    CommLost,
}

impl fmt::Display for Quality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Quality::Good => "good",
            Quality::Bad => "bad",
            Quality::Uncertain => "uncertain",
            Quality::CommLost => "comm_lost",
        };
        f.write_str(s)
    }
}

/// 某一时刻的 tag 值快照：值 + 质量 + 时间戳（unix 毫秒）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagSnapshot {
    pub value: Value,
    pub quality: Quality,
    pub ts_ms: u64,
}

/// 一次增量变更（推送 / 订阅的最小单位）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagChange {
    pub tag: TagId,
    pub value: Value,
    pub quality: Quality,
    pub ts_ms: u64,
}

impl From<(TagId, &TagSnapshot)> for TagChange {
    fn from((tag, s): (TagId, &TagSnapshot)) -> Self {
        TagChange {
            tag,
            value: s.value.clone(),
            quality: s.quality,
            ts_ms: s.ts_ms,
        }
    }
}

/// Tag 的全局唯一标识：`<设备实例id>.<点位id>`，位展开点为
/// `<设备实例id>.<点位id>.b<位号>`。内部 `Arc<str>`，克隆零分配。
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct TagId(Arc<str>);

impl TagId {
    pub fn new(s: impl AsRef<str>) -> Self {
        TagId(Arc::from(s.as_ref()))
    }

    /// `dev1.voltage_ab`
    pub fn join(device: &str, point: &str) -> Self {
        TagId(Arc::from(format!("{device}.{point}")))
    }

    /// `dev1.status.b2`
    pub fn join_bit(device: &str, point: &str, bit: u8) -> Self {
        TagId(Arc::from(format!("{device}.{point}.b{bit}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for TagId {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TagId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for TagId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// 当前 unix 时间戳（毫秒）。
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_id_join() {
        let t = TagId::join("dev1", "voltage_ab");
        assert_eq!(t.as_str(), "dev1.voltage_ab");
        let b = TagId::join_bit("dev1", "status", 2);
        assert_eq!(b.as_str(), "dev1.status.b2");
    }

    #[test]
    fn tag_id_serde_roundtrip() {
        let t = TagId::new("a.b");
        let j = serde_json::to_string(&t).unwrap();
        assert_eq!(j, "\"a.b\"");
        let back: TagId = serde_json::from_str(&j).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn value_conversions() {
        assert_eq!(Value::I64(3).as_f64(), Some(3.0));
        assert_eq!(Value::Bool(true).as_i64(), Some(1));
        assert_eq!(Value::Str("x".into()).as_f64(), None);
    }
}
