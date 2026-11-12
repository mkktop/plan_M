//! TCP 传输：Modbus TCP 网关、串口服务器等场景的字节流连接。
//!
//! 断线由 `LinkError::Closed` 上报，supervisor 负责退避重连；
//! 本模块不做重连。

use async_trait::async_trait;
use hmi_domain::config::{ChannelConfig, TcpParams, TransportConfig};
use hmi_domain::link::{
    Link, LinkError, TransportError, TransportFactory, TransportKind, TransportTarget,
};
use std::any::Any;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, Default, Clone, Copy)]
pub struct TcpFactory;

#[async_trait]
impl TransportFactory for TcpFactory {
    fn kind(&self) -> TransportKind {
        TransportKind::Tcp
    }

    async fn open(&self, channel: &ChannelConfig) -> Result<Box<dyn Link>, TransportError> {
        let TransportConfig::Tcp(params) = &channel.transport else {
            return Err(TransportError::Unavailable(format!(
                "通道 {} 不是 TCP 通道",
                channel.id
            )));
        };
        let link = TcpLink::connect(params)
            .await
            .map_err(TransportError::Open)?;
        Ok(Box::new(link))
    }

    async fn targets(&self) -> Vec<TransportTarget> {
        Vec::new()
    }
}

pub struct TcpLink {
    stream: tokio::sync::Mutex<TcpStream>,
}

impl TcpLink {
    pub async fn connect(params: &TcpParams) -> Result<Self, String> {
        let addr = format!("{}:{}", params.host, params.port);
        let stream = tokio::time::timeout(
            Duration::from_millis(params.connect_timeout_ms as u64),
            TcpStream::connect(&addr),
        )
        .await
        .map_err(|_| format!("连接 {addr} 超时（{}ms）", params.connect_timeout_ms))?
        .map_err(|e| format!("连接 {addr} 失败: {e}"))?;
        let _ = stream.set_nodelay(true);
        Ok(Self {
            stream: tokio::sync::Mutex::new(stream),
        })
    }
}

fn io_err(e: std::io::Error) -> LinkError {
    use std::io::ErrorKind;
    match e.kind() {
        ErrorKind::BrokenPipe
        | ErrorKind::ConnectionReset
        | ErrorKind::ConnectionAborted
        | ErrorKind::NotConnected
        | ErrorKind::UnexpectedEof => LinkError::Closed,
        _ => LinkError::Io(e.to_string()),
    }
}

#[async_trait]
impl Link for TcpLink {
    fn kind(&self) -> TransportKind {
        TransportKind::Tcp
    }

    async fn send(&self, bytes: &[u8]) -> Result<(), LinkError> {
        let mut stream = self.stream.lock().await;
        stream.write_all(bytes).await.map_err(io_err)?;
        stream.flush().await.map_err(io_err)
    }

    async fn recv(&self, max: usize, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        let mut stream = self.stream.lock().await;
        let mut buf = vec![0u8; max.max(1)];
        match tokio::time::timeout(timeout, stream.read(&mut buf)).await {
            Ok(Ok(0)) => Err(LinkError::Closed),
            Ok(Ok(n)) => {
                buf.truncate(n);
                Ok(buf)
            }
            Ok(Err(e)) => Err(io_err(e)),
            Err(_) => Err(LinkError::Timeout),
        }
    }

    async fn drain(&self) {
        let mut stream = self.stream.lock().await;
        let mut buf = [0u8; 512];
        for _ in 0..16 {
            match tokio::time::timeout(Duration::from_millis(2), stream.read(&mut buf)).await {
                Ok(Ok(n)) if n > 0 => continue,
                _ => break,
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn channel_for(addr: std::net::SocketAddr) -> ChannelConfig {
        serde_json::from_value(serde_json::json!({
            "id": "tcp1",
            "name": "测试",
            "transport": "tcp",
            "host": addr.ip().to_string(),
            "port": addr.port(),
            "connectTimeoutMs": 1000
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn echo_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 64];
            let n = sock.read(&mut buf).await.unwrap();
            sock.write_all(&buf[..n]).await.unwrap();
        });

        let channel = channel_for(addr).await;
        let link = TcpFactory.open(&channel).await.unwrap();
        link.send(b"hello").await.unwrap();
        let got = link.recv(64, Duration::from_secs(1)).await.unwrap();
        assert_eq!(got, b"hello");
    }

    #[tokio::test]
    async fn recv_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (_sock, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(2)).await;
        });

        let channel = channel_for(addr).await;
        let link = TcpFactory.open(&channel).await.unwrap();
        let err = link.recv(8, Duration::from_millis(50)).await.unwrap_err();
        assert_eq!(err, LinkError::Timeout);
    }

    #[tokio::test]
    async fn connection_refused() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let channel = channel_for(addr).await;
        assert!(TcpFactory.open(&channel).await.is_err());
    }
}
