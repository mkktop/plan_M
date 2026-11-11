//! Modbus RTU 帧编解码：CRC16、请求构造、应答解析。

/// CRC16-MODBUS（多项式 0xA001，初值 0xFFFF）。线上低字节在前。
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            if crc & 1 == 1 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

/// 在帧尾追加 CRC（低字节在前）。
pub fn append_crc(frame: &mut Vec<u8>) {
    let crc = crc16(frame);
    frame.push((crc & 0xFF) as u8);
    frame.push((crc >> 8) as u8);
}

/// 校验整帧（含尾部 CRC）是否一致。
pub fn check_crc(frame: &[u8]) -> bool {
    if frame.len() < 4 {
        return false;
    }
    let (body, tail) = frame.split_at(frame.len() - 2);
    let crc = crc16(body);
    tail[0] == (crc & 0xFF) as u8 && tail[1] == (crc >> 8) as u8
}

// —— 请求构造 ——

pub fn build_read_request(slave: u8, fc: u8, start: u16, count: u16) -> Vec<u8> {
    let mut f = vec![
        slave,
        fc,
        (start >> 8) as u8,
        (start & 0xFF) as u8,
        (count >> 8) as u8,
        (count & 0xFF) as u8,
    ];
    append_crc(&mut f);
    f
}

pub fn build_write_single_reg(slave: u8, addr: u16, value: u16) -> Vec<u8> {
    let mut f = vec![
        slave,
        0x06,
        (addr >> 8) as u8,
        (addr & 0xFF) as u8,
        (value >> 8) as u8,
        (value & 0xFF) as u8,
    ];
    append_crc(&mut f);
    f
}

pub fn build_write_multi_regs(slave: u8, start: u16, words: &[u16]) -> Vec<u8> {
    let mut f = Vec::with_capacity(9 + words.len() * 2);
    f.push(slave);
    f.push(0x10);
    f.push((start >> 8) as u8);
    f.push((start & 0xFF) as u8);
    f.push((words.len() >> 8) as u8);
    f.push((words.len() & 0xFF) as u8);
    f.push((words.len() * 2) as u8);
    for w in words {
        f.push((w >> 8) as u8);
        f.push((w & 0xFF) as u8);
    }
    append_crc(&mut f);
    f
}

pub fn build_write_single_coil(slave: u8, bit: u16, on: bool) -> Vec<u8> {
    let mut f = vec![
        slave,
        0x05,
        (bit >> 8) as u8,
        (bit & 0xFF) as u8,
        if on { 0xFF } else { 0x00 },
        0x00,
    ];
    append_crc(&mut f);
    f
}

// —— 应答解析 ——

#[derive(Debug, Clone, PartialEq)]
pub enum RespBody {
    /// FC01/02/03/04：寄存器字
    Words(Vec<u16>),
    /// FC01/02：位
    Bits(Vec<bool>),
    /// FC05/06：回显
    Echo,
    /// FC16：写多寄存器确认
    WriteOk,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FrameError {
    #[error("短帧（{0} 字节）")]
    TooShort(usize),
    #[error("CRC 校验失败")]
    BadCrc,
    #[error("从站地址不匹配：请求 {0}，应答 {1}")]
    SlaveMismatch(u8, u8),
    #[error("modbus exception: fc=0x{0:02X} code=0x{1:02X}")]
    Exception(u8, u8),
    #[error("功能码不匹配：请求 0x{0:02X}，应答 0x{1:02X}")]
    FcMismatch(u8, u8),
    #[error("应答长度不合法：{0}")]
    Length(String),
}

/// 解析应答帧。`req` 用于校验从站地址/功能码的一致性。
pub fn parse_response(req: &[u8], resp: &[u8]) -> Result<RespBody, FrameError> {
    if resp.len() < 4 {
        return Err(FrameError::TooShort(resp.len()));
    }
    if !check_crc(resp) {
        return Err(FrameError::BadCrc);
    }
    let slave = resp[0];
    let fc = resp[1];
    if slave != req[0] {
        return Err(FrameError::SlaveMismatch(req[0], slave));
    }
    let req_fc = req[1];
    if fc == req_fc | 0x80 {
        return Err(FrameError::Exception(fc & 0x7F, resp[2]));
    }
    if fc != req_fc {
        return Err(FrameError::FcMismatch(req_fc, fc));
    }
    let body = &resp[2..resp.len() - 2];

    match fc {
        0x01..=0x04 => {
            if body.is_empty() {
                return Err(FrameError::Length("缺字节计数".into()));
            }
            let byte_count = body[0] as usize;
            let data = &body[1..];
            if data.len() != byte_count {
                return Err(FrameError::Length(format!(
                    "字节计数 {byte_count} 与实际 {} 不符",
                    data.len()
                )));
            }
            // 与请求的数量核对
            let req_count = (req[4] as usize) << 8 | req[5] as usize;
            if fc == 0x03 || fc == 0x04 {
                if byte_count != req_count * 2 {
                    return Err(FrameError::Length(format!(
                        "期望 {req_count} 个寄存器（{} 字节），实得 {byte_count} 字节",
                        req_count * 2
                    )));
                }
                Ok(RespBody::Words(
                    data.as_chunks::<2>().0.iter()
                        .map(|c| ((c[0] as u16) << 8) | c[1] as u16)
                        .collect(),
                ))
            } else {
                if byte_count != req_count.div_ceil(8) {
                    return Err(FrameError::Length(format!(
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
        0x05 | 0x06 => {
            if body.len() != 4 {
                return Err(FrameError::Length("FC05/06 应答应为回显（4 字节）".into()));
            }
            Ok(RespBody::Echo)
        }
        0x10 => {
            if body.len() != 4 {
                return Err(FrameError::Length("FC10 应答应为地址+数量".into()));
            }
            Ok(RespBody::WriteOk)
        }
        other => Err(FrameError::Length(format!("不支持的功能码 0x{other:02X}"))),
    }
}

/// 根据请求与应答前 3 字节，推断应答总长度（用于串口按需读取）。
/// 返回 None 表示无法推断（非法帧头）。
pub fn expected_resp_len(header: &[u8]) -> Option<usize> {
    if header.len() < 3 {
        return None;
    }
    let fc = header[1];
    let total = if fc & 0x80 != 0 {
        // 异常帧：addr + fc + code + crc2 = 5
        5
    } else {
        match fc {
            0x01..=0x04 => 5 + header[2] as usize,
            0x05 | 0x06 | 0x10 => 8,
            _ => return None,
        }
    };
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_golden() {
        // 经典向量：01 03 00 00 00 0A 的 CRC 寄存器值 0xCDC5，
        // 线上低字节在前 → 帧尾 C5 CD（见 read_request_golden）
        assert_eq!(crc16(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x0A]), 0xCDC5);
        // 空数据 → 初值
        assert_eq!(crc16(&[]), 0xFFFF);
        // 单字节 0x00 → 0x40BF（CRC-16/MODBUS 参考值）
        assert_eq!(crc16(&[0x00]), 0x40BF);
    }

    #[test]
    fn read_request_golden() {
        let f = build_read_request(0x01, 0x03, 0x0000, 0x000A);
        assert_eq!(f, vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0xC5, 0xCD]);
    }

    #[test]
    fn parse_words_response() {
        // 从站 1 读 2 个保持寄存器 = [0x0064, 0x0ABCD?]，手工构造：
        let req = build_read_request(1, 0x03, 100, 2);
        let mut resp = vec![0x01, 0x03, 0x04, 0x00, 0x64, 0x0A, 0xBC];
        append_crc(&mut resp);
        match parse_response(&req, &resp).unwrap() {
            RespBody::Words(w) => assert_eq!(w, vec![100, 0x0ABC]),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_exception() {
        let req = build_read_request(1, 0x03, 100, 2);
        let mut resp = vec![0x01, 0x83, 0x02]; // 非法数据地址
        append_crc(&mut resp);
        assert_eq!(
            parse_response(&req, &resp),
            Err(FrameError::Exception(0x03, 0x02))
        );
    }

    #[test]
    fn parse_bad_crc() {
        let req = build_read_request(1, 0x03, 100, 2);
        let resp = vec![0x01, 0x03, 0x04, 0x00, 0x64, 0x0A, 0xBC, 0x00, 0x00];
        assert!(matches!(
            parse_response(&req, &resp),
            Err(FrameError::BadCrc)
        ));
    }

    #[test]
    fn parse_bits_response() {
        let req = build_read_request(1, 0x01, 0, 10); // 读 10 个线圈
                                                      // 字节 0b0000_0101 → 线圈 0,2 置位；第二个字节覆盖线圈 8,9
        let mut resp = vec![0x01, 0x01, 0x02, 0x05, 0x01];
        append_crc(&mut resp);
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
    fn expected_len_rules() {
        // 读寄存器应答：addr fc count(=4) → 5+4 = 9
        assert_eq!(expected_resp_len(&[0x01, 0x03, 0x04]), Some(9));
        // 异常帧：恒 5
        assert_eq!(expected_resp_len(&[0x01, 0x83, 0x02]), Some(5));
        // FC06 回显：恒 8
        assert_eq!(expected_resp_len(&[0x01, 0x06, 0x00]), Some(8));
        // 未知功能码
        assert_eq!(expected_resp_len(&[0x01, 0x55, 0x00]), None);
        // 帧头不足
        assert_eq!(expected_resp_len(&[0x01, 0x03]), None);
    }

    #[test]
    fn write_multi_request() {
        let f = build_write_multi_regs(1, 100, &[0x0064, 0x2710]);
        assert_eq!(f.len(), 9 + 4);
        assert_eq!(f[6], 4); // 字节数
        assert_eq!(&f[7..11], &[0x00, 0x64, 0x27, 0x10]);
        assert!(check_crc(&f));
    }
}
