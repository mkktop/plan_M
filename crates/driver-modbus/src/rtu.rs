//! Modbus RTU 帧封装：从站地址 + PDU + CRC16。

use thiserror::Error;

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

pub fn append_crc(frame: &mut Vec<u8>) {
    let crc = crc16(frame);
    frame.push((crc & 0xFF) as u8);
    frame.push((crc >> 8) as u8);
}

pub fn check_crc(frame: &[u8]) -> bool {
    if frame.len() < 4 {
        return false;
    }
    let (body, tail) = frame.split_at(frame.len() - 2);
    let crc = crc16(body);
    tail[0] == (crc & 0xFF) as u8 && tail[1] == (crc >> 8) as u8
}

/// 组装 RTU ADU：`[unit, pdu..., crc_lo, crc_hi]`
pub fn build_adu(unit: u8, pdu: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(pdu.len() + 3);
    frame.push(unit);
    frame.extend_from_slice(pdu);
    append_crc(&mut frame);
    frame
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum RtuError {
    #[error("短帧（{0} 字节）")]
    TooShort(usize),
    #[error("CRC 校验失败")]
    BadCrc,
    #[error("从站地址不匹配：期望 {0}，实得 {1}")]
    UnitMismatch(u8, u8),
}

/// 拆 RTU ADU：校验 CRC 与从站地址，返回 PDU。
pub fn split_adu(unit: u8, frame: &[u8]) -> Result<&[u8], RtuError> {
    if frame.len() < 4 {
        return Err(RtuError::TooShort(frame.len()));
    }
    if !check_crc(frame) {
        return Err(RtuError::BadCrc);
    }
    if frame[0] != unit {
        return Err(RtuError::UnitMismatch(unit, frame[0]));
    }
    Ok(&frame[1..frame.len() - 2])
}

/// 根据应答前 3 字节推断 RTU 应答总长度（用于按需读取）。
pub fn expected_resp_len(header: &[u8]) -> Option<usize> {
    if header.len() < 3 {
        return None;
    }
    let fc = header[1];
    let total = if fc & 0x80 != 0 {
        5
    } else {
        match fc {
            0x01..=0x04 => 5 + header[2] as usize,
            0x05 | 0x06 | 0x0F | 0x10 => 8,
            _ => return None,
        }
    };
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdu;

    #[test]
    fn crc16_golden() {
        assert_eq!(crc16(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x0A]), 0xCDC5);
        assert_eq!(crc16(&[]), 0xFFFF);
        assert_eq!(crc16(&[0x00]), 0x40BF);
    }

    #[test]
    fn read_request_golden() {
        let pdu = pdu::build_read(pdu::FC_READ_HOLDING, 0, 10);
        let frame = build_adu(0x01, &pdu);
        assert_eq!(frame, vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0xC5, 0xCD]);
    }

    #[test]
    fn adu_roundtrip() {
        let pdu = pdu::build_write_single_reg(200, 5678);
        let frame = build_adu(1, &pdu);
        assert_eq!(split_adu(1, &frame).unwrap(), pdu.as_slice());
        assert_eq!(split_adu(2, &frame), Err(RtuError::UnitMismatch(2, 1)));
    }

    #[test]
    fn expected_len_rules() {
        assert_eq!(expected_resp_len(&[0x01, 0x03, 0x04]), Some(9));
        assert_eq!(expected_resp_len(&[0x01, 0x83, 0x02]), Some(5));
        assert_eq!(expected_resp_len(&[0x01, 0x06, 0x00]), Some(8));
        assert_eq!(expected_resp_len(&[0x01, 0x55, 0x00]), None);
        assert_eq!(expected_resp_len(&[0x01, 0x03]), None);
    }
}
