//! 采集运行时核心。
//!
//! - [`hub`]：TagHub 实时库（最新值缓存 + 订阅分发）
//! - [`supervisor`]：通道监督器（串行事务 / 重试 / 设备离线 / 断线重连）
//! - [`service`]：[`GatewayService`] 编排（配置 + 包 + 驱动 + 传输 + 通道）
//! - [`store`]：工程配置存储（v2 JSON 文件，trait 留 SQLite 后路）
//! - [`workspaces`]：设备类型工作区页面覆盖（`workspaces.json`）
//!
//! 本 crate 不依赖任何串口/驱动实现：传输由 [`hmi_domain::link::TransportFactory`]
//! 注入，驱动由 [`hmi_domain::driver::Driver`] 注入 —— gatewayd 负责接线。
//! 轮询编译属于驱动（如 `hmi-driver-modbus`），gateway 只负责调度。

pub mod dashboards;
pub mod hub;
pub mod service;
pub mod store;
pub mod supervisor;
pub mod workspaces;

pub use hub::{Subscription, TagHub};
pub use service::GatewayService;
pub use store::{ConfigStore, JsonConfigStore};
pub use supervisor::{
    spawn_channel, ChannelHandle, ChannelRuntime, ChannelShared, DeviceRuntime, GatewayEvent,
    SupervisorCmd,
};
