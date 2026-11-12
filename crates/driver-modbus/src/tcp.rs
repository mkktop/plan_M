//! Modbus TCP 帧封装：MBAP 头 + PDU。
//!
//! MBAP：transaction id(2) + protocol id(2, 恒 0) + length(2) + unit id(1)
//! length = unit id + PDU 的长度。

use thiserror::Error;

pub const MBAP_HEADER_LEN: usize = 7;
const PROTOCOL_ID: u16 = 0;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum TcpFrameError {
    #[error("短帧（{0} 字节）")]
    TooShort(usize),
    #[error("协议 id 非 0：{0}")]
    BadProtocol(u16),
    #[error("MBAP 长度不合法：{0}")]
    BadLength(u16),
    #[error("单元 id 不匹配：期望 {0}，实得 {1}")]
    UnitMismatch(u8, u8),
}

/// 组装 MBAP 帧。
pub fn build_frame(tid: u16, unit: u8, pdu: &[u8]) -> Vec<u8> {
    let len = (pdu.len() + 1) as u16;
    let mut frame = Vec::with_capacity(MBAP_HEADER_LEN + pdu.len());
    frame.extend_from_slice(&tid.to_be_bytes());
    frame.extend_from_slice(&PROTOCOL_ID.to_be_bytes());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.push(unit);
    frame.extend_from_slice(pdu);
    frame
}

/// 从 MBAP 头解析事务 id 与长度；校验协议 id。
pub fn parse_header(header: &[u8]) -> Result<(u16, u16), TcpFrameError> {
    if header.len() < 6 {
        return Err(TcpFrameError::TooShort(header.len()));
    }
    let tid = u16::from_be_bytes([header[0], header[1]]);
    let proto = u16::from_be_bytes([header[2], header[3]]);
    if proto != PROTOCOL_ID {
        return Err(TcpFrameError::BadProtocol(proto));
    }
    let len = u16::from_be_bytes([header[4], header[5]]);
    if !(2..=254).contains(&len) {
        return Err(TcpFrameError::BadLength(len));
    }
    Ok((tid, len))
}

/// 拆一帧完整 MBAP，校验单元 id，返回 `(tid, unit, pdu)`。
pub fn parse_frame(frame: &[u8], expected_unit: u8) -> Result<(u16, u8, &[u8]), TcpFrameError> {
    let (tid, len) = parse_header(frame)?;
    let total = 6 + len as usize;
    if frame.len() < total {
        return Err(TcpFrameError::TooShort(frame.len()));
    }
    let unit = frame[6];
    if unit != expected_unit {
        return Err(TcpFrameError::UnitMismatch(expected_unit, unit));
    }
    Ok((tid, unit, &frame[MBAP_HEADER_LEN..total]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdu;

    #[test]
    fn frame_roundtrip() {
        let p = pdu::build_read(pdu::FC_READ_HOLDING, 100, 2);
        let frame = build_frame(0x1234, 1, &p);
        assert_eq!(&frame[..2], &[0x12, 0x34]);
        assert_eq!(&frame[2..4], &[0, 0]);
        assert_eq!(&frame[4..6], &(p.len() as u16 + 1).to_be_bytes());
        assert_eq!(frame[6], 1);

        let (tid, unit, got) = parse_frame(&frame, 1).unwrap();
        assert_eq!(tid, 0x1234);
        assert_eq!(unit, 1);
        assert_eq!(got, p.as_slice());
    }

    #[test]
    fn bad_unit_and_protocol() {
        let p = pdu::build_read(pdu::FC_READ_HOLDING, 0, 1);
        let mut frame = build_frame(1, 1, &p);
        assert_eq!(
            parse_frame(&frame, 2),
            Err(TcpFrameError::UnitMismatch(2, 1))
        );
        frame[3] = 1;
        assert_eq!(parse_frame(&frame, 1), Err(TcpFrameError::BadProtocol(1)));
    }
}
