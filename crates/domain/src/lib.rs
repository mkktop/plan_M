//! 领域模型层 —— 整个系统的"语言"。
//!
//! 本 crate 的纪律：
//! - 不依赖 tokio / tauri / axum 等任何运行时或框架；
//! - 只放纯类型、纯函数（编解码、地址解析）与通信"端口"（trait）定义；
//! - 所有 JSON 序列化契约（设备包点表、工程配置、tag 值）都以这里为唯一真相源。
//!
//! 分层：
//! - [`link`]：传输端口（串口/TCP/BLE）+ 传输工厂；
//! - [`driver`]：协议驱动端口（编译点表、轮询、写入、探测）；
//! - [`config`]/[`points`]/[`protocol`]：工程配置与设备包契约；
//! - [`codec`]/[`addr`]：寄存器类协议的编解码与地址工具。

pub mod addr;
pub mod codec;
pub mod config;
pub mod driver;
pub mod link;
pub mod points;
pub mod protocol;
pub mod value;

pub use addr::{RegAddr, RegArea};
pub use codec::{apply_scale, decode_registers, encode_num, ByteOrder, DataType, Num};
pub use config::{
    BleParams, ChannelConfig, ChannelTiming, DeviceAddress, DeviceConfig, Parity, ProjectConfig,
    SerialParams, SimParams, TcpParams, TransportConfig,
};
pub use driver::{
    CompileInput, DevicePlan, Driver, DriverError, PlanPayload, PollClass, PollGroup, Sample,
};
pub use link::{
    read_exact, Link, LinkError, TransportError, TransportFactory, TransportKind, TransportTarget,
};
pub use points::{
    apply_overrides, Access, BitDef, EnumValueDef, PointAddress, PointDef, PointsTable,
    PollGroupDef,
};
pub use protocol::{DriverIssues, ProtocolDecl, ScanRange};
pub use value::{now_ms, Quality, TagChange, TagId, TagSnapshot, Value};
