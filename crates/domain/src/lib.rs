//! 领域模型层 —— 整个系统的"语言"。
//!
//! 本 crate 的纪律：
//! - 不依赖 tokio / tauri / axum 等任何运行时或框架；
//! - 只放纯类型、纯函数（编解码、地址解析）与通信"端口"（trait）定义；
//! - 所有 JSON 序列化契约（设备包点表、工程配置、tag 值）都以这里为唯一真相源。
//!
//! 字节序约定（多字类型）见 [`codec`] 模块文档；Modbus 地址工程记法见 [`addr`]。

pub mod addr;
pub mod codec;
pub mod config;
pub mod driver;
pub mod points;
pub mod transport;
pub mod value;

pub use addr::{RegAddr, RegArea};
pub use codec::{apply_scale, decode_registers, encode_num, ByteOrder, DataType, Num};
pub use config::{DeviceConfig, Parity, ProjectConfig, SerialChannelConfig};
pub use driver::{BlockData, BlockPayload, Driver, DriverError, ReadBlock};
pub use points::{Access, BitDef, PointDef, PointsTable, PollGroupDef, ProtocolDefaults};
pub use transport::{BusError, BusTransport};
pub use value::{now_ms, Quality, TagChange, TagId, TagSnapshot, Value};
