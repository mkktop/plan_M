//! 进程内虚拟传输：无硬件环境下的测试/演示设施。
//!
//! `SimLink` 是字节管道，`SimEndpoint` 在另一端把请求交给应答函数。
//! 协议（如 Modbus）的模拟从站由调用方提供，本 crate 不认识任何协议。

use async_trait::async_trait;
use hmi_domain::config::{ChannelConfig, TransportConfig};
use hmi_domain::link::{
    Link, LinkError, TransportError, TransportFactory, TransportKind, TransportTarget,
};
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// 处理一帧请求字节，返回应答字节；None = 不应答（模拟掉线）。
pub type SimResponder = Arc<dyn Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync>;
/// 为一条通道创建应答器（可持有该通道的模拟从站状态）。
pub type SimResponderFactory = Arc<dyn Fn(&ChannelConfig) -> SimResponder + Send + Sync>;

/// 虚拟传输工厂。按通道 id 缓存应答器，断线重连不会重置模拟从站状态。
pub struct SimFactory {
    factory: SimResponderFactory,
    responders: Mutex<HashMap<String, SimResponder>>,
}

impl SimFactory {
    pub fn new(factory: SimResponderFactory) -> Self {
        Self {
            factory,
            responders: Mutex::new(HashMap::new()),
        }
    }

    /// 回显应答器（测试用）。
    pub fn echo() -> Self {
        Self::new(Arc::new(|_| Arc::new(|req: &[u8]| Some(req.to_vec()))))
    }
}

#[async_trait]
impl TransportFactory for SimFactory {
    fn kind(&self) -> TransportKind {
        TransportKind::Sim
    }

    async fn open(&self, channel: &ChannelConfig) -> Result<Box<dyn Link>, TransportError> {
        if !matches!(&channel.transport, TransportConfig::Sim(_)) {
            return Err(TransportError::Unavailable(format!(
                "通道 {} 不是虚拟通道",
                channel.id
            )));
        }
        let responder = {
            let mut map = self.responders.lock().unwrap();
            map.entry(channel.id.clone())
                .or_insert_with(|| (self.factory)(channel))
                .clone()
        };
        let (link, endpoint) = sim_pair();
        tokio::spawn(async move {
            endpoint.run(move |req| responder(req)).await;
        });
        Ok(Box::new(link))
    }

    async fn targets(&self) -> Vec<TransportTarget> {
        Vec::new()
    }
}

/// 创建一对相连的虚拟端点。
pub fn sim_pair() -> (SimLink, SimEndpoint) {
    let (req_tx, req_rx) = mpsc::channel::<Vec<u8>>(16);
    let (resp_tx, resp_rx) = mpsc::channel::<Vec<u8>>(16);
    (
        SimLink {
            tx: req_tx,
            state: tokio::sync::Mutex::new(LinkState {
                buf: Vec::new(),
                rx: resp_rx,
            }),
        },
        SimEndpoint {
            rx: req_rx,
            tx: resp_tx,
        },
    )
}

struct LinkState {
    /// 上一帧应答中尚未被读走的字节
    buf: Vec<u8>,
    rx: mpsc::Receiver<Vec<u8>>,
}

pub struct SimLink {
    tx: mpsc::Sender<Vec<u8>>,
    state: tokio::sync::Mutex<LinkState>,
}

#[async_trait]
impl Link for SimLink {
    fn kind(&self) -> TransportKind {
        TransportKind::Sim
    }

    async fn send(&self, bytes: &[u8]) -> Result<(), LinkError> {
        self.tx
            .send(bytes.to_vec())
            .await
            .map_err(|_| LinkError::Closed)
    }

    async fn recv(&self, max: usize, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        let mut st = self.state.lock().await;
        if st.buf.is_empty() {
            match tokio::time::timeout(timeout, st.rx.recv()).await {
                Ok(Some(chunk)) => st.buf = chunk,
                Ok(None) => return Err(LinkError::Closed),
                Err(_) => return Err(LinkError::Timeout),
            }
        }
        if st.buf.is_empty() {
            return Err(LinkError::Timeout);
        }
        let n = max.max(1).min(st.buf.len());
        Ok(st.buf.drain(..n).collect())
    }

    async fn drain(&self) {
        let mut st = self.state.lock().await;
        st.buf.clear();
        while st.rx.try_recv().is_ok() {}
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// 从站侧端点。
pub struct SimEndpoint {
    rx: mpsc::Receiver<Vec<u8>>,
    tx: mpsc::Sender<Vec<u8>>,
}

impl SimEndpoint {
    /// 收请求 → `responder` → 有应答则发回。
    pub async fn run<F>(mut self, responder: F)
    where
        F: Fn(&[u8]) -> Option<Vec<u8>> + Send + 'static,
    {
        while let Some(req) = self.rx.recv().await {
            if let Some(resp) = responder(&req) {
                if self.tx.send(resp).await.is_err() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sim_channel() -> ChannelConfig {
        serde_json::from_value(serde_json::json!({
            "id": "sim1", "name": "测试", "transport": "sim"
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn echo_roundtrip() {
        let factory = SimFactory::echo();
        let link = factory.open(&sim_channel()).await.unwrap();
        link.send(b"hello").await.unwrap();
        assert_eq!(
            link.recv(16, Duration::from_secs(1)).await.unwrap(),
            b"hello"
        );
    }

    #[tokio::test]
    async fn recv_chunks_long_response() {
        let factory = SimFactory::new(Arc::new(|_| {
            Arc::new(|_req: &[u8]| Some(b"0123456789".to_vec()))
        }));
        let link = factory.open(&sim_channel()).await.unwrap();
        link.send(b"x").await.unwrap();
        assert_eq!(link.recv(4, Duration::from_secs(1)).await.unwrap(), b"0123");
        assert_eq!(
            link.recv(16, Duration::from_secs(1)).await.unwrap(),
            b"456789"
        );
    }

    #[tokio::test]
    async fn offline_times_out() {
        let factory = SimFactory::new(Arc::new(|_| Arc::new(|_req: &[u8]| None)));
        let link = factory.open(&sim_channel()).await.unwrap();
        link.send(b"x").await.unwrap();
        assert_eq!(
            link.recv(8, Duration::from_millis(30)).await.unwrap_err(),
            LinkError::Timeout
        );
    }
}
