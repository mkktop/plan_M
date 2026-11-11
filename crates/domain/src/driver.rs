//! 通信"端口"定义：协议驱动抽象。
//!
//! 驱动只认"读块 / 写寄存器 / 写线圈"，不做点表合并、不做工程量换算
//! ——合并属于轮询编译器（gateway），换算属于平台层。这样将来 rhai 脚本
//! 驱动可以复用同一套编译与调度。
//!
//! 驱动也不打开/关闭总线：总线生命周期归 ChannelSupervisor，驱动每次
//! 调用拿到 `&dyn BusTransport`。

use crate::addr::RegArea;
use crate::transport::{BusError, BusTransport};
use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;

/// 一块连续读取请求（word 或 bit 计数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadBlock {
    pub area: RegArea,
    /// 0 基起始偏移
    pub start: u16,
    pub count: u16,
}

/// 读块结果载荷。
#[derive(Clone, Debug, PartialEq)]
pub enum BlockPayload {
    Words(Vec<u16>),
    Bits(Vec<bool>),
}

/// 一块读取结果。
#[derive(Clone, Debug, PartialEq)]
pub struct BlockData {
    pub area: RegArea,
    pub start: u16,
    pub payload: BlockPayload,
}

#[derive(Error, Debug, Clone, PartialEq)]
pub enum DriverError {
    #[error("bus: {0}")]
    Bus(#[from] BusError),
    /// 从站返回异常码
    #[error("modbus exception: fc=0x{0:02X} code=0x{1:02X}")]
    Exception(u8, u8),
    /// 应答帧不合法 / 与请求不匹配
    #[error("protocol error: {0}")]
    Proto(String),
}

impl DriverError {
    /// 从站级错误（不意味着总线故障，只影响该从站）。
    pub fn is_slave_level(&self) -> bool {
        matches!(
            self,
            DriverError::Exception(_, _)
                | DriverError::Proto(_)
                | DriverError::Bus(BusError::Timeout)
        )
    }
}

/// 协议驱动端口。第一个实现是 modbus-rtu（hmi-driver-modbus-rtu crate）。
#[async_trait]
pub trait Driver: Send + Sync {
    /// 驱动 id（设备包 manifest 的 protocol.driver 与之匹配）
    fn id(&self) -> &'static str;

    /// 顺序读取若干块。单块超时按 `timeout`；块间静默由本实现保证。
    async fn read_blocks(
        &self,
        bus: &dyn BusTransport,
        slave: u8,
        blocks: &[ReadBlock],
        timeout: Duration,
    ) -> Result<Vec<BlockData>, DriverError>;

    /// 写保持寄存器（1 个字走 FC06，多个走 FC16）。
    async fn write_registers(
        &self,
        bus: &dyn BusTransport,
        slave: u8,
        start: u16,
        words: &[u16],
        timeout: Duration,
    ) -> Result<(), DriverError>;

    /// 写单个线圈。
    async fn write_coil(
        &self,
        bus: &dyn BusTransport,
        slave: u8,
        bit: u16,
        value: bool,
        timeout: Duration,
    ) -> Result<(), DriverError>;

    /// 探测从站是否应答（读 1 个保持寄存器）。Err(BusError::Timeout) 视为不在线。
    async fn probe(
        &self,
        bus: &dyn BusTransport,
        slave: u8,
        timeout: Duration,
    ) -> Result<(), DriverError>;
}
