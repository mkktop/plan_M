//! Modbus 地址模型与"工程记法"解析。
//!
//! # 工程记法（设备手册口径，点表作者直接照手册抄）
//!
//! | 记法 | 区域 | 示例 | 内部偏移 |
//! |------|------|------|----------|
//! | `00001`–`09999` | 线圈 Coil | `00010` | 9 |
//! | `10001`–`19999` | 离散量 Discrete | `10010` | 9 |
//! | `30001`–`39999` | 输入寄存器 Input | `30100` | 99 |
//! | `40001`–`49999` | 保持寄存器 Holding | `40101` | 100 |
//! | `400001`–`465536` | 保持寄存器（6 位扩展） | `400101` | 100 |
//! | `300001`–`365536` | 输入寄存器（6 位扩展） | `300101` | 100 |
//!
//! 另支持显式 0 基记法（大小写不敏感）：`HR:100` / `IR:5` / `C:3` / `DR:7`，
//! 分别对应 Holding/Input/Coil/Discrete + 0 基偏移。

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/// 四个 Modbus 数据区。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegArea {
    /// 线圈（可读写位），FC01/FC05/FC15
    Coil,
    /// 离散量输入（只读位），FC02
    Discrete,
    /// 输入寄存器（只读字），FC04
    Input,
    /// 保持寄存器（可读写字），FC03/FC06/FC16
    Holding,
}

impl RegArea {
    /// 读功能码。
    pub fn read_fc(self) -> u8 {
        match self {
            RegArea::Coil => 0x01,
            RegArea::Discrete => 0x02,
            RegArea::Holding => 0x03,
            RegArea::Input => 0x04,
        }
    }

    /// 是否为位区域（Coil / Discrete）。
    pub fn is_bit_area(self) -> bool {
        matches!(self, RegArea::Coil | RegArea::Discrete)
    }

    /// 是否可写（只有线圈与保持寄存器可写）。
    pub fn writable(self) -> bool {
        matches!(self, RegArea::Coil | RegArea::Holding)
    }

    /// 工程记法前缀字符：'0' 线圈 / '1' 离散 / '3' 输入 / '4' 保持。
    pub fn engineering_leading_digit(self) -> char {
        match self {
            RegArea::Coil => '0',
            RegArea::Discrete => '1',
            RegArea::Input => '3',
            RegArea::Holding => '4',
        }
    }

    /// 显式记法前缀：`HR` / `IR` / `C` / `DR`。
    pub fn explicit_prefix(self) -> &'static str {
        match self {
            RegArea::Coil => "C",
            RegArea::Discrete => "DR",
            RegArea::Input => "IR",
            RegArea::Holding => "HR",
        }
    }
}

impl fmt::Display for RegArea {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.explicit_prefix())
    }
}

/// 一个确定的寄存器/位地址：区域 + 0 基偏移。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RegAddr {
    pub area: RegArea,
    pub offset: u16,
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum AddrError {
    #[error("无法识别的地址记法 `{0}`：支持工程记法（如 40101、30001、00010）或显式记法（如 HR:100、C:3）")]
    Notation(String),
    #[error("地址 `{0}` 的首位数字与区域不匹配或数值超出区域范围")]
    OutOfRange(String),
    #[error("地址偏移超出 65535：`{0}`")]
    OffsetOverflow(String),
}

impl FromStr for RegAddr {
    type Err = AddrError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(AddrError::Notation(s.to_string()));
        }

        // 显式记法：HR:100 / ir:5 / c:3
        if let Some((prefix, num)) = s.split_once(':') {
            let area = match prefix.trim().to_ascii_uppercase().as_str() {
                "HR" | "4X" => RegArea::Holding,
                "IR" | "3X" => RegArea::Input,
                "C" | "0X" => RegArea::Coil,
                "DR" | "1X" => RegArea::Discrete,
                _ => return Err(AddrError::Notation(s.to_string())),
            };
            let offset: u32 = num
                .trim()
                .parse()
                .map_err(|_| AddrError::Notation(s.to_string()))?;
            if offset > u16::MAX as u32 {
                return Err(AddrError::OffsetOverflow(s.to_string()));
            }
            return Ok(RegAddr {
                area,
                offset: offset as u16,
            });
        }

        // 纯数字工程记法
        if !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(AddrError::Notation(s.to_string()));
        }
        let n: u64 = s.parse().map_err(|_| AddrError::Notation(s.to_string()))?;

        let (area, offset) = match (s.len(), s.as_bytes()[0]) {
            // 5 位：首位决定区域
            (5, b'0') => (RegArea::Coil, n.checked_sub(1)),
            (5, b'1') => (RegArea::Discrete, n.checked_sub(10001)),
            (5, b'3') => (RegArea::Input, n.checked_sub(30001)),
            (5, b'4') => (RegArea::Holding, n.checked_sub(40001)),
            // 6 位扩展：保持/输入寄存器
            (6, b'3') => (RegArea::Input, n.checked_sub(300001)),
            (6, b'4') => (RegArea::Holding, n.checked_sub(400001)),
            _ => return Err(AddrError::OutOfRange(s.to_string())),
        };

        match offset {
            Some(off) if off <= u16::MAX as u64 => Ok(RegAddr {
                area,
                offset: off as u16,
            }),
            _ => Err(AddrError::OffsetOverflow(s.to_string())),
        }
    }
}

impl RegAddr {
    /// 转回工程记法字符串（偏移 ≥ 9999 时用显式记法避免歧义）。
    pub fn to_engineering(self) -> String {
        if self.offset >= 9999 {
            return format!("{}:{}", self.area.explicit_prefix(), self.offset);
        }
        let leading = self.area.engineering_leading_digit();
        match self.area {
            RegArea::Coil => format!("{:05}", self.offset as u64 + 1),
            _ => format!("{leading}{:04}", (self.offset as u64 % 10000) + 1),
        }
    }
}

impl fmt::Display for RegAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.area, self.offset)
    }
}

// —— serde：JSON 里以字符串形式出现（"40101" / "HR:100"）——

impl Serialize for RegAddr {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_engineering())
    }
}

impl<'de> Deserialize<'de> for RegAddr {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        RegAddr::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_engineering_5digit() {
        let a: RegAddr = "40101".parse().unwrap();
        assert_eq!(
            a,
            RegAddr {
                area: RegArea::Holding,
                offset: 100
            }
        );
        let a: RegAddr = "30001".parse().unwrap();
        assert_eq!(a.area, RegArea::Input);
        assert_eq!(a.offset, 0);
        let a: RegAddr = "00001".parse().unwrap();
        assert_eq!(a.area, RegArea::Coil);
        assert_eq!(a.offset, 0);
        let a: RegAddr = "10005".parse().unwrap();
        assert_eq!(a.area, RegArea::Discrete);
        assert_eq!(a.offset, 4);
    }

    #[test]
    fn parse_engineering_6digit() {
        let a: RegAddr = "400101".parse().unwrap();
        assert_eq!(a.area, RegArea::Holding);
        assert_eq!(a.offset, 100);
        let a: RegAddr = "300101".parse().unwrap();
        assert_eq!(a.area, RegArea::Input);
        assert_eq!(a.offset, 100);
    }

    #[test]
    fn parse_explicit() {
        let a: RegAddr = "HR:100".parse().unwrap();
        assert_eq!(a.area, RegArea::Holding);
        assert_eq!(a.offset, 100);
        let a: RegAddr = "ir:5".parse().unwrap();
        assert_eq!(a.area, RegArea::Input);
        assert_eq!(a.offset, 5);
        let a: RegAddr = "C:3".parse().unwrap();
        assert_eq!(a.area, RegArea::Coil);
        assert_eq!(a.offset, 3);
        let a: RegAddr = "DR:7".parse().unwrap();
        assert_eq!(a.area, RegArea::Discrete);
        assert_eq!(a.offset, 7);
    }

    #[test]
    fn parse_errors() {
        assert!("40101x".parse::<RegAddr>().is_err());
        assert!("".parse::<RegAddr>().is_err());
        assert!("50001".parse::<RegAddr>().is_err()); // 首位 5 无对应区域
        assert!("40001".parse::<RegAddr>().is_ok());
        assert!("HR:70000".parse::<RegAddr>().is_err()); // 偏移溢出
        assert!("XX:1".parse::<RegAddr>().is_err());
    }

    #[test]
    fn engineering_roundtrip() {
        for s in ["40101", "30001", "00010", "10010"] {
            let a: RegAddr = s.parse().unwrap();
            assert_eq!(a.to_engineering(), s, "roundtrip failed for {s}");
        }
        // 6 位扩展记法与 5 位记法是同一地址；输出统一为 5 位形式
        let a6: RegAddr = "400101".parse().unwrap();
        let a5: RegAddr = "40101".parse().unwrap();
        assert_eq!(a6, a5);
        assert_eq!(a6.to_engineering(), "40101");
        // 偏移 9999 起切换为显式记法，但语义等价
        let a: RegAddr = "HR:12345".parse().unwrap();
        let b: RegAddr = a.to_engineering().parse().unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn serde_as_string() {
        #[derive(serde::Deserialize, serde::Serialize)]
        struct W {
            r: RegAddr,
        }
        let w: W = serde_json::from_str(r#"{"r":"40101"}"#).unwrap();
        assert_eq!(w.r.offset, 100);
        let j = serde_json::to_string(&w).unwrap();
        assert!(j.contains("40101"));
        // 非法地址 → 反序列化报错（点表校验的第一道门）
        assert!(serde_json::from_str::<W>(r#"{"r":"nope"}"#).is_err());
    }

    #[test]
    fn area_helpers() {
        assert_eq!(RegArea::Holding.read_fc(), 0x03);
        assert_eq!(RegArea::Input.read_fc(), 0x04);
        assert_eq!(RegArea::Coil.read_fc(), 0x01);
        assert_eq!(RegArea::Discrete.read_fc(), 0x02);
        assert!(RegArea::Coil.is_bit_area());
        assert!(!RegArea::Holding.is_bit_area());
        assert!(RegArea::Holding.writable());
        assert!(!RegArea::Input.writable());
    }
}
