//! 内存 Modbus 从站：PDU 应答器 + RTU/MBAP 适配。
//!
//! 测试与 `gatewayd sim-tcp` 演示共用；虚拟传输层（transport-sim）只需要
//! 一个 `Fn(&[u8]) -> Option<Vec<u8>>` 应答函数。

use crate::{pdu, rtu, tcp};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 内存 Modbus 从站模拟器（内部可变，可跨任务共享）。
pub struct SimSlave {
    holding: std::sync::Mutex<HashMap<u16, u16>>,
    input: std::sync::Mutex<HashMap<u16, u16>>,
    coils: std::sync::Mutex<HashMap<u16, bool>>,
    discrete: std::sync::Mutex<HashMap<u16, bool>>,
    /// false = 不应答任何请求（模拟掉线）
    online: AtomicBool,
    /// 应答前人工延迟（毫秒），模拟慢设备
    delay_ms: AtomicU64,
    /// 收到的请求数（测试断言用）
    pub request_count: AtomicU64,
}

impl Default for SimSlave {
    fn default() -> Self {
        Self {
            holding: Default::default(),
            input: Default::default(),
            coils: Default::default(),
            discrete: Default::default(),
            online: AtomicBool::new(true),
            delay_ms: AtomicU64::new(0),
            request_count: AtomicU64::new(0),
        }
    }
}

impl SimSlave {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_holding(mut self, addr: u16, v: u16) -> Self {
        self.holding.get_mut().unwrap().insert(addr, v);
        self
    }

    pub fn set_input(mut self, addr: u16, v: u16) -> Self {
        self.input.get_mut().unwrap().insert(addr, v);
        self
    }

    pub fn set_coil(mut self, addr: u16, v: bool) -> Self {
        self.coils.get_mut().unwrap().insert(addr, v);
        self
    }

    pub fn set_discrete(mut self, addr: u16, v: bool) -> Self {
        self.discrete.get_mut().unwrap().insert(addr, v);
        self
    }

    pub fn holding_value(&self, addr: u16) -> Option<u16> {
        self.holding.lock().unwrap().get(&addr).copied()
    }

    pub fn input_value(&self, addr: u16) -> Option<u16> {
        self.input.lock().unwrap().get(&addr).copied()
    }

    pub fn coil_value(&self, addr: u16) -> Option<bool> {
        self.coils.lock().unwrap().get(&addr).copied()
    }

    /// 上/下线（离线 = 对任何请求不应答）。
    pub fn set_online(&self, online: bool) {
        self.online.store(online, Ordering::Relaxed);
    }

    pub fn set_delay_ms(&self, ms: u64) {
        self.delay_ms.store(ms, Ordering::Relaxed);
    }

    /// 处理一个请求 PDU。返回应答 PDU；None 表示不应答（掉线）。
    pub fn respond_pdu(&self, _unit: u8, req: &[u8]) -> Option<Vec<u8>> {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        if !self.online.load(Ordering::Relaxed) {
            return None;
        }
        let delay = self.delay_ms.load(Ordering::Relaxed);
        if delay > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }

        let fc = req.first().copied().unwrap_or(0);
        let parsed = match pdu::parse_request(req) {
            Ok(r) => r,
            Err(_) => return Some(vec![fc | 0x80, 0x01]),
        };

        let resp = match parsed {
            pdu::Request::Read { fc, start, count } => {
                if fc == pdu::FC_READ_HOLDING || fc == pdu::FC_READ_INPUT {
                    let map = if fc == pdu::FC_READ_HOLDING {
                        self.holding.lock().unwrap()
                    } else {
                        self.input.lock().unwrap()
                    };
                    let mut bytes = Vec::with_capacity(count as usize * 2);
                    for i in 0..count {
                        let v = map.get(&(start + i)).copied().unwrap_or(0);
                        bytes.push((v >> 8) as u8);
                        bytes.push((v & 0xFF) as u8);
                    }
                    let mut r = vec![fc, bytes.len() as u8];
                    r.extend_from_slice(&bytes);
                    r
                } else {
                    let map = if fc == pdu::FC_READ_COILS {
                        self.coils.lock().unwrap()
                    } else {
                        self.discrete.lock().unwrap()
                    };
                    let n = (count as usize).div_ceil(8);
                    let mut bytes = vec![0u8; n];
                    for i in 0..count as usize {
                        let addr = start + i as u16;
                        if map.get(&addr).copied().unwrap_or(false) {
                            bytes[i / 8] |= 1 << (i % 8);
                        }
                    }
                    let mut r = vec![fc, n as u8];
                    r.extend_from_slice(&bytes);
                    r
                }
            }
            pdu::Request::WriteCoil { bit, on } => {
                self.coils.lock().unwrap().insert(bit, on);
                req.to_vec()
            }
            pdu::Request::WriteReg { addr, value } => {
                self.holding.lock().unwrap().insert(addr, value);
                req.to_vec()
            }
            pdu::Request::WriteCoils { start, bits } => {
                let mut map = self.coils.lock().unwrap();
                for (i, b) in bits.iter().enumerate() {
                    map.insert(start + i as u16, *b);
                }
                let count = bits.len() as u16;
                vec![
                    pdu::FC_WRITE_COILS,
                    (start >> 8) as u8,
                    (start & 0xFF) as u8,
                    (count >> 8) as u8,
                    (count & 0xFF) as u8,
                ]
            }
            pdu::Request::WriteRegs { start, words } => {
                let mut map = self.holding.lock().unwrap();
                for (i, w) in words.iter().enumerate() {
                    map.insert(start + i as u16, *w);
                }
                let count = words.len() as u16;
                vec![
                    pdu::FC_WRITE_REGS,
                    (start >> 8) as u8,
                    (start & 0xFF) as u8,
                    (count >> 8) as u8,
                    (count & 0xFF) as u8,
                ]
            }
        };
        Some(resp)
    }

    /// 处理一帧 RTU ADU。
    pub fn respond_rtu(&self, frame: &[u8]) -> Option<Vec<u8>> {
        if frame.len() < 4 || !rtu::check_crc(frame) {
            return None;
        }
        let unit = frame[0];
        let pdu = &frame[1..frame.len() - 2];
        self.respond_pdu(unit, pdu)
            .map(|resp| rtu::build_adu(unit, &resp))
    }

    /// 处理一个 MBAP 请求，返回完整 MBAP 应答。
    pub fn respond_mbap(&self, tid: u16, unit: u8, pdu: &[u8]) -> Option<Vec<u8>> {
        self.respond_pdu(unit, pdu)
            .map(|resp| tcp::build_frame(tid, unit, &resp))
    }
}

/// 便捷：把 `SimSlave` 包成虚拟传输层需要的应答函数。
pub fn rtu_responder(
    slave: Arc<SimSlave>,
) -> impl Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync + 'static {
    move |req| slave.respond_rtu(req)
}

/// 在一条已接受的 TCP 连接上服务 Modbus TCP 请求。
pub async fn serve_tcp_stream(mut stream: TcpStream, slave: Arc<SimSlave>) -> std::io::Result<()> {
    loop {
        let mut header = [0u8; 6];
        match stream.read_exact(&mut header).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e),
        }
        let (tid, len) = match tcp::parse_header(&header) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };
        let mut body = vec![0u8; len as usize];
        stream.read_exact(&mut body).await?;
        let unit = body[0];
        let pdu = &body[1..];
        if let Some(resp) = slave.respond_mbap(tid, unit, pdu) {
            stream.write_all(&resp).await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtu_roundtrip() {
        let slave = SimSlave::new().set_holding(100, 2200);
        let pdu = pdu::build_read(pdu::FC_READ_HOLDING, 100, 1);
        let req = rtu::build_adu(1, &pdu);
        let resp = slave.respond_rtu(&req).unwrap();
        let resp_pdu = rtu::split_adu(1, &resp).unwrap();
        assert_eq!(
            pdu::parse_response(&pdu, resp_pdu).unwrap(),
            pdu::RespBody::Words(vec![2200])
        );
    }

    #[test]
    fn offline_returns_none() {
        let slave = SimSlave::new();
        slave.set_online(false);
        let pdu = pdu::build_read(pdu::FC_READ_HOLDING, 0, 1);
        let req = rtu::build_adu(1, &pdu);
        assert!(slave.respond_rtu(&req).is_none());
    }

    #[test]
    fn write_fc06() {
        let slave = SimSlave::new();
        let pdu = pdu::build_write_single_reg(200, 5678);
        let req = rtu::build_adu(1, &pdu);
        let resp = slave.respond_rtu(&req).unwrap();
        assert_eq!(resp, req);
        assert_eq!(slave.holding_value(200), Some(5678));
    }

    #[test]
    fn exception_on_unknown_fc() {
        let slave = SimSlave::new();
        let resp = slave.respond_pdu(1, &[0x42, 0, 0, 0, 1]).unwrap();
        assert_eq!(resp, vec![0xC2, 0x01]);
    }

    #[test]
    fn mbap_roundtrip() {
        let slave = SimSlave::new().set_holding(100, 2200);
        let pdu = pdu::build_read(pdu::FC_READ_HOLDING, 100, 1);
        let resp = slave.respond_mbap(7, 1, &pdu).unwrap();
        let (tid, unit, resp_pdu) = tcp::parse_frame(&resp, 1).unwrap();
        assert_eq!(tid, 7);
        assert_eq!(unit, 1);
        assert_eq!(
            pdu::parse_response(&pdu, resp_pdu).unwrap(),
            pdu::RespBody::Words(vec![2200])
        );
    }
}
