//! SIM 通道的虚拟总线网络：每个服务实例一张网络表（按通道 id 复用共享总线）。
//!
//! 用途：`gatewayd serve --sim` 演示模式与 WS 冒烟测试 —— 无硬件跑通全链路。
//! 通道重连时复用同一条虚拟总线（演示语义）。

use hmi_domain::transport::BusTransport;
use hmi_driver_modbus_rtu::{sim_pair, spawn_slave_endpoint, SimSlave};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// 服务实例级的 SIM 网络：通道 id → 共享虚拟总线。
#[derive(Default)]
pub struct SimNetwork {
    buses: Mutex<HashMap<String, Arc<dyn BusTransport>>>,
}

impl SimNetwork {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 取（或创建）某通道的共享虚拟总线。
    pub fn bus_for(&self, channel_id: &str) -> Arc<dyn BusTransport> {
        let mut reg = self.buses.lock().unwrap();
        reg.entry(channel_id.to_string())
            .or_insert_with(|| {
                let (transport, endpoint) = sim_pair();
                let slave = demo_slave();
                spawn_slave_endpoint(endpoint, slave);
                Arc::new(transport)
            })
            .clone()
    }
}

/// 预置演示数据的模拟电表（两个从站地址共享同一应答器，值相同）。
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
