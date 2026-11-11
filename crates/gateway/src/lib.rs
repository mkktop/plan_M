//! 采集运行时核心。
//!
//! - [`hub`]：TagHub 实时库（最新值缓存 + 订阅分发）
//! - [`poll`]：轮询编译器（点表 → 读块合并 + 解码映射）
//! - [`supervisor`]：通道监督器（串行事务 / 重试 / 从站离线 / 断线重连）
//! - [`service`]：[`GatewayService`] 编排（配置 + 包 + 驱动 + 通道）
//! - [`store`]：工程配置存储（v1 JSON 文件，trait 留 SQLite 后路）
//!
//! 本 crate 不依赖任何串口/驱动实现：总线打开由外部注入（[`supervisor::BusOpener`]），
//! 驱动通过 domain 的 [`hmi_domain::driver::Driver`] 端口注入 —— gatewayd 负责接线。

pub mod hub;
pub mod poll;
pub mod service;
pub mod store;
pub mod supervisor;

pub use hub::{Subscription, TagHub};
pub use poll::{compile, DevicePollPlan, PollGroupPlan};
pub use service::GatewayService;
pub use store::{ConfigStore, JsonConfigStore};
pub use supervisor::{
    spawn_channel, BusOpener, ChannelHandle, ChannelRuntime, ChannelShared, DeviceRuntime,
    GatewayEvent, SupervisorCmd,
};
