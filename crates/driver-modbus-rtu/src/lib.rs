//! Modbus RTU 驱动（第一个协议驱动实现）。
//!
//! - [`frame`]：RTU 帧编解码（CRC16、请求构造、应答解析）
//! - [`serial`]：串口传输（真实硬件）与串口枚举
//! - [`sim`]：虚拟总线 + 内存从站（测试/演示）
//! - [`ModbusRtuDriver`]：domain `Driver` 端口的实现

pub mod driver;
pub mod frame;
pub mod serial;
pub mod sim;

pub use driver::ModbusRtuDriver;
pub use serial::{list_serial_ports, SerialPortDesc, SerialTransport};
pub use sim::{sim_pair, spawn_slave_endpoint, SimEndpoint, SimSlave, SimTransport};
