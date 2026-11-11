//! 虚拟总线：无硬件环境下的测试/演示设施。
//!
//! `sim_pair()` 返回 `(SimTransport, SimEndpoint)`：
//! - `SimTransport` 实现 [`BusTransport`]，给驱动/调度器用；
//! - `SimEndpoint` 在另一端收请求、交给你给的应答函数（通常是 [`SimSlave`]）。
//!
//! `SimSlave` 是一个内存 Modbus 从站，可预设寄存器值、模拟离线（不应答）。

use crate::frame;
use hmi_domain::transport::{BusError, BusTransport};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

/// 创建一对相连的虚拟总线端点。
pub fn sim_pair() -> (SimTransport, SimEndpoint) {
    let (req_tx, req_rx) = mpsc::channel::<Vec<u8>>(8);
    let (resp_tx, resp_rx) = mpsc::channel::<Vec<u8>>(8);
    (
        SimTransport {
            req_tx,
            resp_rx: tokio::sync::Mutex::new(resp_rx),
        },
        SimEndpoint { req_rx, resp_tx },
    )
}

/// 驱动侧的虚拟总线传输。
pub struct SimTransport {
    req_tx: mpsc::Sender<Vec<u8>>,
    // transact 是 &self 而 mpsc::Receiver 独占 —— 用 tokio 异步锁
    // （可跨 await 持有）串行化各事务的"等应答"段。
    resp_rx: tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>,
}

#[async_trait::async_trait]
impl BusTransport for SimTransport {
    async fn transact(&self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, BusError> {
        self.req_tx
            .send(request.to_vec())
            .await
            .map_err(|_| BusError::Closed)?;
        let mut rx = self.resp_rx.lock().await;
        match tokio::time::timeout(timeout, rx.recv()).await {
            Ok(Some(bytes)) => Ok(bytes),
            Ok(None) => Err(BusError::Closed),
            Err(_) => Err(BusError::Timeout),
        }
    }
}

/// 从站侧端点：驱动发来的每帧交给应答函数。
pub struct SimEndpoint {
    req_rx: mpsc::Receiver<Vec<u8>>,
    resp_tx: mpsc::Sender<Vec<u8>>,
}

impl SimEndpoint {
    /// 运行事件循环：收请求 → `responder` → 有应答则发回。
    /// responder 返回 None 模拟"不应答"（从站掉线）。
    pub async fn run<F>(mut self, mut responder: F)
    where
        F: FnMut(&[u8]) -> Option<Vec<u8>> + Send,
    {
        while let Some(req) = self.req_rx.recv().await {
            if let Some(resp) = responder(&req) {
                if self.resp_tx.send(resp).await.is_err() {
                    break;
                }
            }
        }
    }
}

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

    /// 处理一帧请求。返回 None 表示不应答。
    pub fn respond(&self, req: &[u8]) -> Option<Vec<u8>> {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        if !self.online.load(Ordering::Relaxed) {
            return None;
        }
        if req.len() < 4 || !frame::check_crc(req) {
            return None; // 坏帧不应答
        }
        let delay = self.delay_ms.load(Ordering::Relaxed);
        if delay > 0 {
            std::thread::sleep(Duration::from_millis(delay));
        }

        let slave = req[0];
        let fc = req[1];
        let start = ((req[2] as u16) << 8) | req[3] as u16;
        let count = ((req[4] as u16) << 8) | req[5] as u16;

        let resp: Vec<u8> = match fc {
            0x01..=0x04 => {
                let area_words = fc == 0x03 || fc == 0x04;
                if area_words {
                    let map = if fc == 0x03 {
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
                    let mut r = vec![slave, fc, bytes.len() as u8];
                    r.extend_from_slice(&bytes);
                    r
                } else {
                    let map = if fc == 0x01 {
                        self.coils.lock().unwrap()
                    } else {
                        self.discrete.lock().unwrap()
                    };
                    let n = (count as usize).div_ceil(8);
                    let mut bytes = vec![0u8; n];
                    for i in 0..count as usize {
                        let addr = start + i as u16;
                        let bit = map.get(&addr).copied().unwrap_or(false);
                        if bit {
                            bytes[i / 8] |= 1 << (i % 8);
                        }
                    }
                    let mut r = vec![slave, fc, n as u8];
                    r.extend_from_slice(&bytes);
                    r
                }
            }
            0x05 => {
                let on = req[4] == 0xFF;
                self.coils.lock().unwrap().insert(start, on);
                req[..req.len() - 2].to_vec()
            }
            0x06 => {
                let value = ((req[4] as u16) << 8) | req[5] as u16;
                self.holding.lock().unwrap().insert(start, value);
                req[..req.len() - 2].to_vec()
            }
            0x0F => {
                // 写多线圈（较少用，简单支持）
                let byte_count = req[6] as usize;
                let mut map = self.coils.lock().unwrap();
                for i in 0..count as usize {
                    let byte_idx = i / 8;
                    if byte_idx < byte_count {
                        let bit = (req[7 + byte_idx] >> (i % 8)) & 1 == 1;
                        map.insert(start + i as u16, bit);
                    }
                }
                let mut r = vec![slave, fc];
                r.extend_from_slice(&req[2..6]);
                r
            }
            0x10 => {
                let mut map = self.holding.lock().unwrap();
                for i in 0..count as usize {
                    let b = 7 + i * 2;
                    if b + 1 < req.len() {
                        let v = ((req[b] as u16) << 8) | req[b + 1] as u16;
                        map.insert(start + i as u16, v);
                    }
                }
                let mut r = vec![slave, fc];
                r.extend_from_slice(&req[2..6]);
                r
            }
            _ => {
                // 非法功能码 → 异常 01
                vec![slave, fc | 0x80, 0x01]
            }
        };

        let mut resp = resp;
        frame::append_crc(&mut resp);
        Some(resp)
    }
}

/// 便捷构造：起一个后台任务跑 `SimSlave` 应答循环。
pub fn spawn_slave_endpoint(
    endpoint: SimEndpoint,
    slave: Arc<SimSlave>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        endpoint.run(move |req| slave.respond(req)).await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::build_read_request;

    #[tokio::test]
    async fn sim_roundtrip() {
        let (transport, endpoint) = sim_pair();
        let slave = Arc::new(SimSlave::new().set_holding(100, 2200));
        spawn_slave_endpoint(endpoint, slave.clone());

        let req = build_read_request(1, 0x03, 100, 1);
        let resp = transport
            .transact(&req, Duration::from_millis(200))
            .await
            .unwrap();
        let parsed = crate::frame::parse_response(&req, &resp).unwrap();
        assert_eq!(parsed, crate::frame::RespBody::Words(vec![2200]));
    }

    #[tokio::test]
    async fn sim_offline_times_out() {
        let (transport, endpoint) = sim_pair();
        let slave = Arc::new(SimSlave::new());
        spawn_slave_endpoint(endpoint, slave.clone());

        slave.set_online(false);
        let req = build_read_request(1, 0x03, 0, 1);
        let r = transport.transact(&req, Duration::from_millis(150)).await;
        assert_eq!(r, Err(BusError::Timeout));
    }

    #[test]
    fn slave_exception_on_unknown_fc() {
        let slave = SimSlave::new();
        let mut req = vec![0x01, 0x42, 0x00, 0x00, 0x00, 0x01];
        frame::append_crc(&mut req);
        let resp = slave.respond(&req).unwrap();
        assert_eq!(resp[1], 0xC2);
        assert_eq!(resp[2], 0x01);
    }

    #[test]
    fn slave_write_fc06() {
        let slave = SimSlave::new();
        let req = crate::frame::build_write_single_reg(1, 200, 5678);
        let resp = slave.respond(&req).unwrap();
        assert_eq!(resp, req);
        assert_eq!(slave.holding_value(200), Some(5678));
    }
}
