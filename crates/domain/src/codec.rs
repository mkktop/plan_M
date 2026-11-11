//! 寄存器字序列 ↔ 数值 的编解码：数据类型 × 字节序 × BCD。
//!
//! # 字节序约定
//!
//! 多字值（32/64 位）跨 2/4 个寄存器传输，各家设备排列不同。以 32 位值为例，
//! 4 个字节按"大端语义"（MSB 在前）记作 A B C D，Modbus 按字（16 位，线上高字节在前）
//! 传输。设线上先到的寄存器为 w0、后到为 w1：
//!
//! | ByteOrder | 线上字节序（w0 → w1） | 语义 | 常见来源 |
//! |-----------|----------------------|------|----------|
//! | `abcd` | A B C D | 标准大端，w0 是高字 | 大多数欧系/标准设备 |
//! | `cdab` | C D A B | 字交换（w0 是低字） | 大量国产电表 |
//! | `badc` | B A D C | 字内字节交换 | 部分 Modicon / 特殊固件 |
//! | `dcba` | D C B A | 完全反转（小端） | 少数嵌入式实现 |
//!
//! 64 位值按同样规则推广：`cdab` 相邻字两两交换，`dcba` 全部 4 字反转，`badc`
//! 每字内字节交换。
//!
//! 单字（16 位）类型只有两种实际排列：`abcd`/`cdab` = 高字节在前（标准），
//! `badc`/`dcba` = 低字节在前（小端字）。

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::value::Value;

/// 解码的中间数值：整数路径不丢精度（u64 电度计数器），浮点路径保 f64。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Num {
    I(i64),
    F(f64),
}

impl Num {
    pub fn as_f64(self) -> f64 {
        match self {
            Num::I(v) => v as f64,
            Num::F(v) => v,
        }
    }
}

impl From<Num> for Value {
    fn from(n: Num) -> Self {
        match n {
            Num::I(v) => Value::I64(v),
            Num::F(v) => Value::F64(v),
        }
    }
}

/// 点表数据类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum DataType {
    /// 无符号 16 位（1 寄存器）
    #[default]
    U16,
    /// 有符号 16 位（1 寄存器）
    I16,
    /// 无符号 32 位（2 寄存器）
    U32,
    /// 有符号 32 位（2 寄存器）
    I32,
    /// 无符号 64 位（4 寄存器）
    U64,
    /// IEEE754 单精度（2 寄存器）
    F32,
    /// IEEE754 双精度（4 寄存器）
    F64,
    /// BCD 16 位：4 位十进制数字，1 寄存器
    Bcd16,
    /// BCD 32 位：8 位十进制数字，2 寄存器
    Bcd32,
}

impl DataType {
    /// 占用寄存器（16 位字）数。
    pub fn word_count(self) -> usize {
        match self {
            DataType::U16 | DataType::I16 | DataType::Bcd16 => 1,
            DataType::U32 | DataType::I32 | DataType::F32 | DataType::Bcd32 => 2,
            DataType::U64 | DataType::F64 => 4,
        }
    }

    pub fn is_bcd(self) -> bool {
        matches!(self, DataType::Bcd16 | DataType::Bcd32)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DataType::U16 => "u16",
            DataType::I16 => "i16",
            DataType::U32 => "u32",
            DataType::I32 => "i32",
            DataType::U64 => "u64",
            DataType::F32 => "f32",
            DataType::F64 => "f64",
            DataType::Bcd16 => "bcd16",
            DataType::Bcd32 => "bcd32",
        }
    }
}

/// 字节序。见模块文档的约定表。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ByteOrder {
    /// 标准大端（高字在前）
    #[default]
    Abcd,
    /// 字内字节交换
    Badc,
    /// 字交换（低字在前）—— 国产电表最常见
    Cdab,
    /// 完全反转
    Dcba,
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum CodecError {
    #[error("需要 {expected} 个寄存器，实得 {actual}")]
    WordCount { expected: usize, actual: usize },
    #[error("数值 {0:?} 超出类型 {1} 的表示范围")]
    Overflow(f64, &'static str),
    #[error("BCD 编码非法（存在大于 9 的位）")]
    BadBcd,
    #[error("类型不匹配：{0}")]
    TypeMismatch(String),
}

/// 计算"规范字节序（大端）第 i 字节"应取自"线上字节"的哪个下标。
/// 线上字节 = words 依次展开，每字 [高, 低]。
fn canonical_perm(bo: ByteOrder, words: usize) -> Vec<usize> {
    let n = words * 2;
    (0..n)
        .map(|i| match bo {
            ByteOrder::Abcd => i,
            // 相邻字两两交换（cdab）：canonical 字 j ← 线上字 (j^1)，字内字节序不变
            // （32 位：w1 w0；64 位：w1 w0 w3 w2）。单字时无可交换，等同 abcd。
            ByteOrder::Cdab => {
                let j = i / 2;
                let k = i % 2;
                let pair = if words > 1 { j ^ 1 } else { j };
                pair * 2 + k
            }
            // 字内字节交换（badc）
            ByteOrder::Badc => {
                let j = i / 2;
                let k = i % 2;
                j * 2 + (1 - k)
            }
            // 完全反转（dcba）
            ByteOrder::Dcba => n - 1 - i,
        })
        .collect()
}

fn words_to_wire_bytes(words: &[u16]) -> Vec<u8> {
    let mut b = Vec::with_capacity(words.len() * 2);
    for w in words {
        b.push((w >> 8) as u8);
        b.push((w & 0xFF) as u8);
    }
    b
}

fn wire_bytes_to_words(bytes: &[u8]) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| ((c[0] as u16) << 8) | c[1] as u16)
        .collect()
}

/// 把线上寄存器序列重排为"规范大端字节"。
fn to_canonical(bo: ByteOrder, words: &[u16]) -> Vec<u8> {
    let wire = words_to_wire_bytes(words);
    let perm = canonical_perm(bo, words.len());
    perm.iter().map(|&src| wire[src]).collect()
}

/// 把"规范大端字节"重排回线上寄存器序列。
fn from_canonical(bo: ByteOrder, canonical: &[u8]) -> Vec<u16> {
    let words = canonical.len() / 2;
    let perm = canonical_perm(bo, words);
    let mut wire = vec![0u8; canonical.len()];
    for (i, &c) in canonical.iter().enumerate() {
        wire[perm[i]] = c;
    }
    wire_bytes_to_words(&wire)
}

/// 解码：寄存器序列 → 数值。
pub fn decode_registers(dt: DataType, bo: ByteOrder, words: &[u16]) -> Result<Num, CodecError> {
    let expected = dt.word_count();
    if words.len() != expected {
        return Err(CodecError::WordCount {
            expected,
            actual: words.len(),
        });
    }

    let c = to_canonical(bo, words);

    if dt.is_bcd() {
        return bcd_to_num(&c);
    }

    Ok(match dt {
        DataType::U16 => Num::I(u16::from_be_bytes([c[0], c[1]]) as i64),
        DataType::I16 => Num::I(i16::from_be_bytes([c[0], c[1]]) as i64),
        DataType::U32 => Num::I(u32::from_be_bytes([c[0], c[1], c[2], c[3]]) as i64),
        DataType::I32 => Num::I(i32::from_be_bytes([c[0], c[1], c[2], c[3]]) as i64),
        DataType::U64 => {
            // u64 可能超出 i64 正区间；溢出时落到 f64（保值不保精度，质量戳由上层定）
            let v = u64::from_be_bytes(c.try_into().unwrap());
            match i64::try_from(v) {
                Ok(v) => Num::I(v),
                Err(_) => Num::F(v as f64),
            }
        }
        DataType::F32 => Num::F(f32::from_be_bytes([c[0], c[1], c[2], c[3]]) as f64),
        DataType::F64 => Num::F(f64::from_be_bytes(c.try_into().unwrap())),
        DataType::Bcd16 | DataType::Bcd32 => unreachable!(),
    })
}

fn bcd_to_num(bytes: &[u8]) -> Result<Num, CodecError> {
    let mut v: i64 = 0;
    for &b in bytes {
        let hi = b >> 4;
        let lo = b & 0x0F;
        if hi > 9 || lo > 9 {
            return Err(CodecError::BadBcd);
        }
        v = v * 100 + (hi as i64) * 10 + lo as i64;
    }
    Ok(Num::I(v))
}

fn num_to_bcd(v: u64, digits: usize) -> Result<Vec<u8>, CodecError> {
    let s = v.to_string();
    if s.len() > digits {
        return Err(CodecError::Overflow(v as f64, "bcd"));
    }
    let padded = format!("{:0>width$}", s, width = digits);
    Ok(padded
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| ((c[0] - b'0') << 4) | (c[1] - b'0'))
        .collect())
}

/// 编码：数值 → 寄存器序列（写路径）。
pub fn encode_num(dt: DataType, bo: ByteOrder, num: Num) -> Result<Vec<u16>, CodecError> {
    let canonical: Vec<u8> = if dt.is_bcd() {
        let digits = if dt == DataType::Bcd16 { 4 } else { 8 };
        let v = match num {
            Num::I(v) if v >= 0 => v as u64,
            _ => return Err(CodecError::Overflow(num.as_f64(), dt.as_str())),
        };
        num_to_bcd(v, digits)?
    } else {
        match (dt, num) {
            (DataType::U16, Num::I(v)) if (0..=u16::MAX as i64).contains(&v) => {
                (v as u16).to_be_bytes().to_vec()
            }
            (DataType::I16, Num::I(v)) if (i16::MIN as i64..=i16::MAX as i64).contains(&v) => {
                (v as i16).to_be_bytes().to_vec()
            }
            (DataType::U32, Num::I(v)) if (0..=u32::MAX as i64).contains(&v) => {
                (v as u32).to_be_bytes().to_vec()
            }
            (DataType::I32, Num::I(v)) if (i32::MIN as i64..=i32::MAX as i64).contains(&v) => {
                (v as i32).to_be_bytes().to_vec()
            }
            (DataType::U64, Num::I(v)) if v >= 0 => (v as u64).to_be_bytes().to_vec(),
            (DataType::F32, n) => (n.as_f64() as f32).to_be_bytes().to_vec(),
            (DataType::F64, n) => n.as_f64().to_be_bytes().to_vec(),
            (dt, n) => {
                return Err(CodecError::Overflow(n.as_f64(), dt.as_str()));
            }
        }
    };
    Ok(from_canonical(bo, &canonical))
}

/// 应用工程量换算：`value * scale + offset`。
/// - 无 scale 且无 offset：原样返回（整数保持 I64，不损失精度）；
/// - 有任一：返回 F64。
pub fn apply_scale(num: Num, scale: Option<f64>, offset: Option<f64>) -> Value {
    let (s, o) = (scale.unwrap_or(1.0), offset.unwrap_or(0.0));
    if scale.is_none() && offset.is_none() {
        return num.into();
    }
    Value::F64(num.as_f64() * s + o)
}

/// 从 16 位状态字中取某一位。
pub fn extract_bit(word: u16, bit: u8) -> bool {
    if bit >= 16 {
        return false;
    }
    (word >> bit) & 1 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: fn(u16, u16) -> Vec<u16> = |a, b| vec![a, b];

    #[test]
    fn u16_single_word() {
        // 0x1234：abcd → 4660；badc（小端字）→ 0x3412
        assert_eq!(
            decode_registers(DataType::U16, ByteOrder::Abcd, &[0x1234]).unwrap(),
            Num::I(0x1234)
        );
        assert_eq!(
            decode_registers(DataType::U16, ByteOrder::Badc, &[0x1234]).unwrap(),
            Num::I(0x3412)
        );
    }

    #[test]
    fn u32_orders() {
        // 数值 0xAABBCCDD：abcd → w0=0xAABB w1=0xCCDD；cdab → w0=0xCCDD w1=0xAABB；
        // badc → w0=0xBBAA w1=0xDDCC；dcba → w0=0xDDCC w1=0xBBAA
        let v = 0xAABBCCDDu32;
        let w = W(0xAABB, 0xCCDD);
        assert_eq!(
            decode_registers(DataType::U32, ByteOrder::Abcd, &w).unwrap(),
            Num::I(v as i64)
        );
        assert_eq!(
            decode_registers(DataType::U32, ByteOrder::Cdab, &W(0xCCDD, 0xAABB)).unwrap(),
            Num::I(v as i64)
        );
        assert_eq!(
            decode_registers(DataType::U32, ByteOrder::Badc, &W(0xBBAA, 0xDDCC)).unwrap(),
            Num::I(v as i64)
        );
        assert_eq!(
            decode_registers(DataType::U32, ByteOrder::Dcba, &W(0xDDCC, 0xBBAA)).unwrap(),
            Num::I(v as i64)
        );
    }

    #[test]
    fn i32_negative() {
        // -100.0 的 i32 表示
        let w = W(0xFFFF, 0xFF9C);
        assert_eq!(
            decode_registers(DataType::I32, ByteOrder::Abcd, &w).unwrap(),
            Num::I(-100)
        );
    }

    #[test]
    fn f32_cdab_common_meter() {
        // 220.5f32 的字节：43 5C 80 00 → abcd w=0x435C,0x8000；cdab 交换
        let f = 220.5f32;
        let b = f.to_be_bytes();
        let w_ab = W(
            ((b[0] as u16) << 8) | b[1] as u16,
            ((b[2] as u16) << 8) | b[3] as u16,
        );
        assert_eq!(
            decode_registers(DataType::F32, ByteOrder::Abcd, &w_ab).unwrap(),
            Num::F(220.5)
        );
        let w_cd = W(w_ab[1], w_ab[0]);
        assert_eq!(
            decode_registers(DataType::F32, ByteOrder::Cdab, &w_cd).unwrap(),
            Num::F(220.5)
        );
    }

    #[test]
    fn u64_four_words() {
        let v = 0x1122334455667788u64;
        let w = vec![0x1122, 0x3344, 0x5566, 0x7788];
        assert_eq!(
            decode_registers(DataType::U64, ByteOrder::Abcd, &w).unwrap(),
            Num::I(v as i64)
        );
        // cdab：相邻字两两交换
        let w2 = vec![0x3344, 0x1122, 0x7788, 0x5566];
        assert_eq!(
            decode_registers(DataType::U64, ByteOrder::Cdab, &w2).unwrap(),
            Num::I(v as i64)
        );
        // dcba：字节完全反转（线上字节倒序 = 0x88 77 66 55 44 33 22 11）
        let w3 = vec![0x8877, 0x6655, 0x4433, 0x2211];
        assert_eq!(
            decode_registers(DataType::U64, ByteOrder::Dcba, &w3).unwrap(),
            Num::I(v as i64)
        );
    }

    #[test]
    fn bcd_decode() {
        // BCD32 是 8 位十进制数字：0x0012 0x3456 → "00123456" → 123456
        assert_eq!(
            decode_registers(DataType::Bcd32, ByteOrder::Abcd, &W(0x0012, 0x3456)).unwrap(),
            Num::I(123456)
        );
        // 0x1234 0x5600 → "12345600"
        assert_eq!(
            decode_registers(DataType::Bcd32, ByteOrder::Abcd, &W(0x1234, 0x5600)).unwrap(),
            Num::I(12345600)
        );
        assert_eq!(
            decode_registers(DataType::Bcd16, ByteOrder::Abcd, &[0x0479]).unwrap(),
            Num::I(479)
        );
        // 非法 BCD（ nibble > 9 ）
        assert_eq!(
            decode_registers(DataType::Bcd16, ByteOrder::Abcd, &[0x04FA]),
            Err(CodecError::BadBcd)
        );
    }

    #[test]
    fn word_count_enforced() {
        assert_eq!(
            decode_registers(DataType::U32, ByteOrder::Abcd, &[0x1]),
            Err(CodecError::WordCount {
                expected: 2,
                actual: 1
            })
        );
    }

    #[test]
    fn encode_roundtrip_all_types() {
        // 每种类型取各自的合法值
        let cases = [
            (DataType::U16, 60000),
            (DataType::I16, -12345),
            (DataType::U32, 1234567),
            (DataType::I32, -1234567),
            (DataType::U64, 1234567890123),
        ];
        for (dt, v) in cases {
            for bo in [
                ByteOrder::Abcd,
                ByteOrder::Badc,
                ByteOrder::Cdab,
                ByteOrder::Dcba,
            ] {
                let n = Num::I(v);
                let words = encode_num(dt, bo, n)
                    .unwrap_or_else(|e| panic!("{dt:?} {bo:?} encode failed: {e:?}"));
                let back = decode_registers(dt, bo, &words)
                    .unwrap_or_else(|e| panic!("{dt:?} {bo:?} decode failed: {e:?}"));
                assert_eq!(back, n, "{dt:?} {bo:?} roundtrip");
            }
        }
        // 精确 roundtrip 一组：70000 = 0x00011170，cdab 低字在前
        let words = encode_num(DataType::U32, ByteOrder::Cdab, Num::I(70000)).unwrap();
        assert_eq!(words, W(0x1170, 0x0001));
        assert_eq!(
            decode_registers(DataType::U32, ByteOrder::Cdab, &words).unwrap(),
            Num::I(70000)
        );
    }

    #[test]
    fn bcd_encode() {
        let words = encode_num(DataType::Bcd16, ByteOrder::Abcd, Num::I(479)).unwrap();
        assert_eq!(words, vec![0x0479]);
        // BCD32 补零到 8 位数字：123456 → "00123456"
        let words = encode_num(DataType::Bcd32, ByteOrder::Abcd, Num::I(123456)).unwrap();
        assert_eq!(words, W(0x0012, 0x3456));
        assert!(encode_num(DataType::Bcd16, ByteOrder::Abcd, Num::I(12345)).is_err());
        assert!(encode_num(DataType::Bcd16, ByteOrder::Abcd, Num::I(-1)).is_err());
    }

    #[test]
    fn scale_application() {
        // 无换算：保持整数
        assert_eq!(apply_scale(Num::I(2200), None, None), Value::I64(2200));
        // 有 scale：浮点
        assert_eq!(
            apply_scale(Num::I(2200), Some(0.1), None),
            Value::F64(220.0)
        );
        assert_eq!(
            apply_scale(Num::I(100), Some(0.1), Some(2.0)),
            Value::F64(12.0)
        );
    }

    #[test]
    fn bits() {
        let w = 0b0000_0000_0000_0101u16;
        assert!(extract_bit(w, 0));
        assert!(!extract_bit(w, 1));
        assert!(extract_bit(w, 2));
        assert!(!extract_bit(w, 17));
    }
}
