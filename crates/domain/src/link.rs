//! 传输层端口：一条通道对应一个 [`Link`]。
//!
//! `Link` 只负责字节收发，不认识任何协议帧格式：
//! - 串口 / TCP：`send` + `recv` 的字节流；
//! - BLE 透传：同样按字节流实现；
//! - 原生 GATT：通过 [`Link::as_any`] 暴露传输私有句柄，由对应协议驱动下转型使用。
//!
//! 协议的帧头、长度、CRC、MBAP 等全部由驱动实现，传输层不再解析。

use crate::config::ChannelConfig;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::time::Duration;
use thiserror::Error;

/// 传输种类。
///
/// 新增一种传输 = 新增一个 [`TransportFactory`] 实现并在宿主注册；
/// gateway / supervisor / packaging 主干不需要改动。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransportKind {
    Serial,
    Tcp,
    /// 进程内虚拟总线（演示/测试），复用 RTU 字节流语义。
    Sim,
    /// 预留：BLE 透传或原生 GATT。
    Ble,
}

impl TransportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TransportKind::Serial => "serial",
            TransportKind::Tcp => "tcp",
            TransportKind::Sim => "sim",
            TransportKind::Ble => "ble",
        }
    }
}

impl std::fmt::Display for TransportKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum LinkError {
    /// 应答超时（从站不在线 / 波特率不对 / 帧损坏）
    #[error("response timeout")]
    Timeout,
    /// 连接已关闭（串口拔出、TCP 断开、BLE 断开）→ 触发通道重连
    #[error("link closed")]
    Closed,
    /// 传输级 I/O 错误 → 触发通道重连
    #[error("link io error: {0}")]
    Io(String),
}

/// 一条已打开的连接。
#[async_trait]
pub trait Link: Send + Sync + Any {
    fn kind(&self) -> TransportKind;

    /// 发送完整字节序列。
    async fn send(&self, bytes: &[u8]) -> Result<(), LinkError>;

    /// 读取 `1..=max` 字节。`timeout` 内没有任何数据则返回 [`LinkError::Timeout`]。
    ///
    /// 返回长度可能小于 `max`（流式语义），由协议层按帧格式凑齐。
    async fn recv(&self, max: usize, timeout: Duration) -> Result<Vec<u8>, LinkError>;

    /// 丢弃接收缓冲中的残帧（事务超时后调用）。
    async fn drain(&self);

    /// 传输私有句柄（BLE GATT 等非字节协议使用）。
    fn as_any(&self) -> &dyn Any;
}

/// 传输层可发现的目标（串口、BLE 外设等）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportTarget {
    /// 用于创建通道/设备的稳定 id（串口名、BLE MAC 等）
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    /// 硬件 id（如 `VID_1A86&PID_7523`），用于 COM 号漂移后重新匹配
    #[serde(default)]
    pub hw_id: Option<String>,
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum TransportError {
    /// 宿主未注册该传输（如尚未实现的 BLE）
    #[error("transport unavailable: {0}")]
    Unavailable(String),
    /// 打开失败（串口占用、TCP 拒绝连接等）
    #[error("open failed: {0}")]
    Open(String),
}

/// 传输工厂：宿主为每种 [`TransportKind`] 注册一个。
#[async_trait]
pub trait TransportFactory: Send + Sync {
    fn kind(&self) -> TransportKind;

    /// 打开一条通道。TCP/BLE 需要异步连接，所以这里是 async。
    async fn open(&self, channel: &ChannelConfig) -> Result<Box<dyn Link>, TransportError>;

    /// 该传输可发现的候选目标（默认无）。
    async fn targets(&self) -> Vec<TransportTarget> {
        Vec::new()
    }
}

/// 按帧读取：从 `Link` 上凑满 `n` 字节，总时限由 `deadline` 控制。
pub async fn read_exact(
    link: &dyn Link,
    n: usize,
    deadline: std::time::Instant,
) -> Result<Vec<u8>, LinkError> {
    let mut buf = Vec::with_capacity(n);
    while buf.len() < n {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(LinkError::Timeout);
        }
        let chunk = link.recv(n - buf.len(), remaining).await?;
        if chunk.is_empty() {
            return Err(LinkError::Closed);
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}
