//! 通信"端口"定义：总线传输抽象。
//!
//! 语义约定：一次 `transact` = 完整的一问一答（发请求帧 → 等应答帧）。
//! 半双工总线的串行化由上层（ChannelSupervisor）保证，本 trait 的实现
//! 只需保证单次事务的原子性。

use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum BusError {
    /// 应答超时（从站不在线 / 波特率不对 / 帧损坏）
    #[error("response timeout")]
    Timeout,
    /// 串口级 I/O 错误（通常意味着 USB 拔出，需要重连）
    #[error("bus io error: {0}")]
    Io(String),
    /// 传输已关闭
    #[error("transport closed")]
    Closed,
}

/// 总线传输：串口（SerialTransport）与虚拟总线（SimTransport）共同实现。
#[async_trait]
pub trait BusTransport: Send + Sync {
    /// 发送请求并等待应答。超时返回 [`BusError::Timeout`]。
    async fn transact(&self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, BusError>;
}
