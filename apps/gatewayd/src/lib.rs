//! gatewayd 库层：CLI 之外的可测试部件（路由、WS DTO 映射、仪表盘/工作区存储、
//! SIM 网络、驱动/传输装配、日志）。

pub mod cmd_pkg;
pub mod cmd_poll;
pub mod cmd_scan;
pub mod dashboards;
pub mod dto;
pub mod logging;
pub mod serve;
pub mod sim_tcp;
pub mod simnet;
pub mod workspaces;

use hmi_domain::driver::Driver;
use hmi_domain::link::TransportFactory;
use std::sync::Arc;

/// 宿主版本（设备包 minHostVersion 与之比较）。
pub const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 宿主注册的驱动。新增协议在这里加一行。
pub fn drivers() -> Vec<Arc<dyn Driver>> {
    vec![Arc::new(hmi_driver_modbus::ModbusDriver::new())]
}

/// 宿主注册的传输。新增传输在这里加一行。
pub fn transports() -> Vec<Arc<dyn TransportFactory>> {
    vec![
        Arc::new(hmi_transport_serial::SerialFactory),
        Arc::new(hmi_transport_tcp::TcpFactory),
        simnet::sim_factory(),
    ]
}

/// 默认数据目录：`%APPDATA%/gatewayd`（可被 --data-dir 覆盖）。
pub fn default_data_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("gatewayd")
}
