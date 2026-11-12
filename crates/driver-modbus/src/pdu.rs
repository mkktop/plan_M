//! Modbus PDU：功能码语义，不含从站地址、CRC 或 MBAP。
//!
//! RTU 与 TCP 两种帧封装共用这里的请求构造与应答解析。

use thiserror::Error;

pub const FC_READ_COILS: u8 = 0x01;
pub const FC_READ_DISCRETE: u8 = 0x02;
pub const FC_READ_HOLDING: u8 = 0x03;
pub const FC_READ_INPUT: u8 = 0x04;
pub const FC_WRITE_COIL: u8 = 0x05;
pub const FC_WRITE_REG: u8 = 0x06;
pub const FC_WRITE_COILS: u8 = 0x0F;
pub const FC_WRITE_REGS: u8 = 0x10;

/// 读请求 PDU：`[fc, start_hi, start_lo, count_hi, count_lo]`
pub fn build_read(fc: u8, start: u16, count: u16) -> Vec<u8> {
    vec![
        fc,
        (start >> 8) as u8,
        (start & 0xFF) as u8,
        (count >> 8) as u8,
        (count & 0xFF) as u8,
    ]
}

/// 写单寄存器（FC06）
pub fn build_write_single_reg(addr: u16, value: u16) -> Vec<u8> {
    vec![
        FC_WRITE_REG,
        (addr >> 8) as u8,
        (addr & 0xFF) as u8,
        (value >> 8) as u8,
        (value & 0xFF) as u8,
    ]
}

/// 写多寄存器（FC16）
pub fn build_write_multi_regs(start: u16, words: &[u16]) -> Vec<u8> {
    let mut pdu = Vec::with_capacity(6 + words.len() * 2);
    pdu.push(FC_WRITE_REGS);
    pdu.push((start >> 8) as u8);
    pdu.push((start & 0xFF) as u8);
    pdu.push((words.len() >> 8) as u8);
    pdu.push((words.len() & 0xFF) as u8);
    pdu.push((words.len() * 2) as u8);
    for w in words {
        pdu.push((w >> 8) as u8);
        pdu.push((w & 0xFF) as u8);
    }
    pdu
}

/// 写单线圈（FC05）
pub fn build_write_single_coil(bit: u16, on: bool) -> Vec<u8> {
    vec![
        FC_WRITE_COIL,
        (bit >> 8) as u8,
        (bit & 0xFF) as u8,
        if on { 0xFF } else { 0x00 },
        0x00,
    ]
}

/// 写多线圈（FC15）
pub fn build_write_multi_coils(start: u16, bits: &[bool]) -> Vec<u8> {
    let n = bits.len();
    let bytes = n.div_ceil(8);
    let mut pdu = Vec::with_capacity(6 + bytes);
    pdu.push(FC_WRITE_COILS);
    pdu.push((start >> 8) as u8);
    pdu.push((start & 0xFF) as u8);
    pdu.push((n >> 8) as u8);
    pdu.push((n & 0xFF) as u8);
    pdu.push(bytes as u8);
    let mut data = vec![0u8; bytes];
    for (i, &b) in bits.iter().enumerate() {
        if b {
            data[i / 8] |= 1 << (i % 8);
        }
    }
    pdu.extend_from_slice(&data);
    pdu
}

#[derive(Debug, Clone, PartialEq)]
pub enum RespBody {
    /// FC01/02/03/04
    Words(Vec<u16>),
    Bits(Vec<bool>),
    /// FC05/06 回显
    Echo,
    /// FC15/16 确认
    WriteOk,
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum PduError {
    #[error("短 PDU（{0} 字节）")]
    TooShort(usize),
    /// 从站返回异常码（fc, code）
    #[error("modbus exception: fc=0x{0:02X} code=0x{1:02X}")]
    Exception(u8, u8),
    #[error("功能码不匹配：请求 0x{0:02X}，应答 0x{1:02X}")]
    FcMismatch(u8, u8),
    #[error("应答长度不合法：{0}")]
    Length(String),
    #[error("不支持的功能码 0x{0:02X}")]
    UnsupportedFc(u8),
}

/// 解析应答 PDU。`req` 只用于取请求功能码与数量。
pub fn parse_response(req: &[u8], resp: &[u8]) -> Result<RespBody, PduError> {
    if req.is_empty() {
        return Err(PduError::TooShort(0));
    }
    if resp.len() < 2 {
        return Err(PduError::TooShort(resp.len()));
    }
    let req_fc = req[0];
    let fc = resp[0];
    if fc == req_fc | 0x80 {
        return Err(PduError::Exception(req_fc, resp[1]));
    }
    if fc != req_fc {
        return Err(PduError::FcMismatch(req_fc, fc));
    }
    let body = &resp[1..];

    match fc {
        FC_READ_COILS | FC_READ_DISCRETE | FC_READ_HOLDING | FC_READ_INPUT => {
            if body.is_empty() {
                return Err(PduError::Length("缺字节计数".into()));
            }
            if req.len() < 5 {
                return Err(PduError::TooShort(req.len()));
            }
            let req_count = ((req[3] as usize) << 8) | req[4] as usize;
            let byte_count = body[0] as usize;
            let data = &body[1..];
            if data.len() != byte_count {
                return Err(PduError::Length(format!(
                    "字节计数 {byte_count} 与实际 {} 不符",
                    data.len()
                )));
            }
            if fc == FC_READ_HOLDING || fc == FC_READ_INPUT {
                if byte_count != req_count * 2 {
                    return Err(PduError::Length(format!(
                        "期望 {req_count} 个寄存器（{} 字节），实得 {byte_count} 字节",
                        req_count * 2
                    )));
                }
                Ok(RespBody::Words(
                    data.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| ((c[0] as u16) << 8) | c[1] as u16)
                        .collect(),
                ))
            } else {
                if byte_count != req_count.div_ceil(8) {
                    return Err(PduError::Length(format!(
                        "期望 {req_count} 个位（{} 字节），实得 {byte_count} 字节",
                        req_count.div_ceil(8)
                    )));
                }
                let mut bits = Vec::with_capacity(req_count);
                for (i, &b) in data.iter().enumerate() {
                    for k in 0..8 {
                        if i * 8 + k < req_count {
                            bits.push((b >> k) & 1 == 1);
                        }
                    }
                }
                Ok(RespBody::Bits(bits))
            }
        }
        FC_WRITE_COIL | FC_WRITE_REG => {
            if body.len() != 4 {
                return Err(PduError::Length("FC05/06 应答应为 4 字节回显".into()));
            }
            Ok(RespBody::Echo)
        }
        FC_WRITE_COILS | FC_WRITE_REGS => {
            if body.len() != 4 {
                return Err(PduError::Length("FC15/16 应答应为地址+数量".into()));
            }
            Ok(RespBody::WriteOk)
        }
        other => Err(PduError::UnsupportedFc(other)),
    }
}

/// 解析一个请求 PDU（模拟从站用）。
pub fn parse_request(pdu: &[u8]) -> Result<Request, PduError> {
    if pdu.is_empty() {
        return Err(PduError::TooShort(0));
    }
    let fc = pdu[0];
    let need = |n: usize| -> Result<(), PduError> {
        if pdu.len() < n {
            Err(PduError::TooShort(pdu.len()))
        } else {
            Ok(())
        }
    };
    let word = |hi: u8, lo: u8| ((hi as u16) << 8) | lo as u16;
    match fc {
        FC_READ_COILS | FC_READ_DISCRETE | FC_READ_HOLDING | FC_READ_INPUT => {
            need(5)?;
            Ok(Request::Read {
                fc,
                start: word(pdu[1], pdu[2]),
                count: word(pdu[3], pdu[4]),
            })
        }
        FC_WRITE_COIL => {
            need(5)?;
            Ok(Request::WriteCoil {
                bit: word(pdu[1], pdu[2]),
                on: pdu[3] == 0xFF,
            })
        }
        FC_WRITE_REG => {
            need(5)?;
            Ok(Request::WriteReg {
                addr: word(pdu[1], pdu[2]),
                value: word(pdu[3], pdu[4]),
            })
        }
        FC_WRITE_COILS => {
            need(6)?;
            let count = word(pdu[3], pdu[4]) as usize;
            let byte_count = pdu[5] as usize;
            need(6 + byte_count)?;
            let mut bits = Vec::with_capacity(count);
            for i in 0..count {
                let byte = pdu[6 + i / 8];
                bits.push((byte >> (i % 8)) & 1 == 1);
            }
            Ok(Request::WriteCoils {
                start: word(pdu[1], pdu[2]),
                bits,
            })
        }
        FC_WRITE_REGS => {
            need(6)?;
            let count = word(pdu[3], pdu[4]) as usize;
            let byte_count = pdu[5] as usize;
            if byte_count != count * 2 {
                return Err(PduError::Length(format!(
                    "FC16 字节计数 {byte_count} 与数量 {count} 不符"
                )));
            }
            need(6 + byte_count)?;
            let words = pdu[6..6 + byte_count]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| word(c[0], c[1]))
                .collect();
            Ok(Request::WriteRegs {
                start: word(pdu[1], pdu[2]),
                words,
            })
        }
        other => Err(PduError::UnsupportedFc(other)),
    }
}

/// 模拟从站解析出的请求。
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Read { fc: u8, start: u16, count: u16 },
    WriteCoil { bit: u16, on: bool },
    WriteReg { addr: u16, value: u16 },
    WriteCoils { start: u16, bits: Vec<bool> },
    WriteRegs { start: u16, words: Vec<u16> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_words_response() {
        let req = build_read(FC_READ_HOLDING, 100, 2);
        let resp = vec![0x03, 0x04, 0x00, 0x64, 0x0A, 0xBC];
        match parse_response(&req, &resp).unwrap() {
            RespBody::Words(w) => assert_eq!(w, vec![100, 0x0ABC]),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_bits_response() {
        let req = build_read(FC_READ_COILS, 0, 10);
        let resp = vec![0x01, 0x02, 0x05, 0x01];
        match parse_response(&req, &resp).unwrap() {
            RespBody::Bits(b) => {
                assert_eq!(b.len(), 10);
                assert!(b[0] && !b[1] && b[2]);
                assert!(b[8] && !b[9]);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_exception() {
        let req = build_read(FC_READ_HOLDING, 100, 2);
        assert_eq!(
            parse_response(&req, &[0x83, 0x02]),
            Err(PduError::Exception(0x03, 0x02))
        );
    }

    #[test]
    fn parse_write_echo() {
        let req = build_write_single_reg(200, 5678);
        let resp = vec![0x06, 0x00, 0xC8, 0x16, 0x2E];
        assert_eq!(parse_response(&req, &resp), Ok(RespBody::Echo));
    }

    #[test]
    fn request_parse_roundtrip() {
        let pdu = build_write_multi_regs(100, &[1, 2, 3]);
        assert_eq!(
            parse_request(&pdu).unwrap(),
            Request::WriteRegs {
                start: 100,
                words: vec![1, 2, 3]
            }
        );
    }
}
