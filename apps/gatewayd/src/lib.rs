//! gatewayd 库层：CLI 之外的可测试部件（路由、SIM 网络、总线工厂、日志）。

pub mod cmd_pkg;
pub mod cmd_poll;
pub mod cmd_scan;
pub mod logging;
pub mod serve;
pub mod simnet;

use hmi_driver_modbus_rtu::SerialTransport;
use hmi_gateway::supervisor::{BusOpener, SharedBus};
use std::sync::Arc;

/// 宿主版本（设备包 minHostVersion 与之比较）。
pub const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 总线工厂：SIM 通道 → 本服务实例的共享虚拟总线；其余 → 串口（带 COM 号漂移回退）。
pub fn bus_opener(sim: Arc<simnet::SimNetwork>) -> BusOpener {
    Arc::new(move |cfg| {
        if cfg.port.eq_ignore_ascii_case("SIM") {
            let bus = sim.bus_for(&cfg.id);
            Ok(Box::new(SharedBus(bus)) as Box<dyn hmi_domain::transport::BusTransport>)
        } else {
            open_serial_with_hw_fallback(cfg)
                .map(|t| Box::new(t) as Box<dyn hmi_domain::transport::BusTransport>)
        }
    })
}

/// 打开串口；失败且配置了硬件 ID 时，按 VID/PID 找回漂移后的 COM 号再试一次。
fn open_serial_with_hw_fallback(
    cfg: &hmi_domain::config::SerialChannelConfig,
) -> Result<SerialTransport, String> {
    match SerialTransport::open(cfg) {
        Ok(t) => Ok(t),
        Err(e) => {
            if let Some(hw) = &cfg.hw_id {
                for p in hmi_driver_modbus_rtu::list_serial_ports() {
                    if p.hw_id.as_deref() == Some(hw.as_str()) && p.port_name != cfg.port {
                        tracing::info!(
                            from = %cfg.port,
                            to = %p.port_name,
                            "COM 号漂移，按硬件 ID 重新匹配"
                        );
                        let mut c2 = cfg.clone();
                        c2.port = p.port_name.clone();
                        return SerialTransport::open(&c2);
                    }
                }
            }
            Err(e)
        }
    }
}

/// 默认数据目录：`%APPDATA%/gatewayd`（可被 --data-dir 覆盖）。
pub fn default_data_dir() -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("gatewayd")
}
