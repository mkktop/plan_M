//! Modbus 驱动：一个协议实现同时支持 RTU 与 MBAP 两种帧封装。
//!
//! - [`pdu`]：功能码语义（无地址/CRC/MBAP）
//! - [`rtu`]：RTU ADU（CRC）
//! - [`tcp`]：MBAP 帧
//! - [`compile`]：点表 → 读块合并 + 解码映射（Modbus 专属逻辑）
//! - [`sim`]：内存从站（PDU 应答器 + RTU/MBAP 适配）
//! - [`ModbusDriver`]：domain `Driver` 实现

pub mod compile;
pub mod driver;
pub mod pdu;
pub mod rtu;
pub mod sim;
pub mod tcp;

pub use compile::{Framing, ModbusParams};
pub use driver::ModbusDriver;
pub use sim::{rtu_responder, serve_tcp_stream, SimSlave};
