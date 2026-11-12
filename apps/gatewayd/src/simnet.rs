//! SIM 传输：进程内虚拟总线的演示从站。
//!
//! `SimFactory` 负责为每条 SIM 通道创建一个 Modbus RTU 应答器；
//! 演示数据与 `gatewayd serve --sim` / `sim-tcp` 共用。

use hmi_domain::config::ChannelConfig;
use hmi_domain::link::TransportFactory;
use hmi_driver_modbus::SimSlave;
use hmi_transport_sim::{SimFactory, SimResponder};
use std::sync::Arc;

/// 构造宿主用 SIM 传输工厂（每条通道一个独立的演示从站状态）。
pub fn sim_factory() -> Arc<dyn TransportFactory> {
    Arc::new(SimFactory::new(Arc::new(|_channel: &ChannelConfig| {
        let slave = demo_slave();
        Arc::new(move |req: &[u8]| slave.respond_rtu(req)) as SimResponder
    })))
}

/// 预置演示数据的模拟电表（PM3000 演示包）。
pub fn demo_slave() -> Arc<SimSlave> {
    Arc::new(
        SimSlave::new()
            // PM3000 演示包（examples/packages/pm3000）的寄存器预置值
            .set_holding(100, 2201) // 40101 Uab → 220.1 V
            .set_holding(101, 2198) // 40102 Ubc
            .set_holding(102, 2205) // 40103 Uca
            .set_holding(103, 1523) // 40104 Ia → 15.23 A
            .set_holding(104, 1480) // 40105 Ib
            .set_holding(105, 1495) // 40106 Ic
            .set_holding(106, 0x2710) // 40107 P 低字（cdab）→ 10000
            .set_holding(107, 0x0000) // P 高字 → i32 10000 *0.1 = 1000.0 W
            .set_holding(108, 0x0BB8) // 40109 Q → 3000
            .set_holding(109, 0x0000)
            .set_holding(114, 5000) // 40115 频率 → 50.00 Hz
            .set_holding(199, 0x2710) // 40200 电度低字 → 10000 *0.01 = 100.00 kWh
            .set_holding(200, 0x0000)
            .set_holding(201, 0x1388) // 40202 无功电度 → 50.00 kvarh
            .set_holding(202, 0x0000)
            .set_holding(299, 0x0001) // 40300 状态字：b0=运行
            .set_holding(309, 250), // 40310 设定值
    )
}
