//! Modbus RTU 驱动：把 domain 的 Driver 端口映射到 RTU 帧。

use hmi_domain::addr::RegArea;
use hmi_domain::driver::{BlockData, BlockPayload, Driver, DriverError, ReadBlock};
use hmi_domain::transport::BusTransport;
use std::time::Duration;

use crate::frame::{self, RespBody};

/// 单块读取上限（协议规定）。
const MAX_WORDS_PER_READ: u16 = 125;
const MAX_BITS_PER_READ: u16 = 2000;

impl From<frame::FrameError> for DriverError {
    fn from(e: frame::FrameError) -> Self {
        match e {
            frame::FrameError::Exception(fc, code) => DriverError::Exception(fc, code),
            other => DriverError::Proto(other.to_string()),
        }
    }
}

#[derive(Default)]
pub struct ModbusRtuDriver;

impl ModbusRtuDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl Driver for ModbusRtuDriver {
    fn id(&self) -> &'static str {
        "modbus-rtu"
    }

    async fn read_blocks(
        &self,
        bus: &dyn BusTransport,
        slave: u8,
        blocks: &[ReadBlock],
        timeout: Duration,
    ) -> Result<Vec<BlockData>, DriverError> {
        let mut out = Vec::with_capacity(blocks.len());
        for block in blocks {
            // 防御：超上限的块拆分（编译器正常不会产出）
            let max = if block.area.is_bit_area() {
                MAX_BITS_PER_READ
            } else {
                MAX_WORDS_PER_READ
            };
            let mut start = block.start;
            let mut remaining = block.count;
            while remaining > 0 {
                let n = remaining.min(max);
                let body = self
                    .read_one(bus, slave, block.area, start, n, timeout)
                    .await?;
                out.push(BlockData {
                    area: block.area,
                    start,
                    payload: body,
                });
                remaining -= n;
                start = start.saturating_add(n);
            }
        }
        Ok(out)
    }

    async fn write_registers(
        &self,
        bus: &dyn BusTransport,
        slave: u8,
        start: u16,
        words: &[u16],
        timeout: Duration,
    ) -> Result<(), DriverError> {
        let req = if words.len() == 1 {
            frame::build_write_single_reg(slave, start, words[0])
        } else {
            frame::build_write_multi_regs(slave, start, words)
        };
        let resp = bus.transact(&req, timeout).await?;
        match frame::parse_response(&req, &resp)? {
            RespBody::Echo | RespBody::WriteOk => Ok(()),
            _ => Err(DriverError::Proto("FC06/10 应答类型不匹配".into())),
        }
    }

    async fn write_coil(
        &self,
        bus: &dyn BusTransport,
        slave: u8,
        bit: u16,
        value: bool,
        timeout: Duration,
    ) -> Result<(), DriverError> {
        let req = frame::build_write_single_coil(slave, bit, value);
        let resp = bus.transact(&req, timeout).await?;
        match frame::parse_response(&req, &resp)? {
            RespBody::Echo => Ok(()),
            _ => Err(DriverError::Proto("FC05 应答类型不匹配".into())),
        }
    }

    async fn probe(
        &self,
        bus: &dyn BusTransport,
        slave: u8,
        timeout: Duration,
    ) -> Result<(), DriverError> {
        let req = frame::build_read_request(slave, 0x03, 0, 1);
        let resp = bus.transact(&req, timeout).await?;
        frame::parse_response(&req, &resp)?;
        Ok(())
    }
}

impl ModbusRtuDriver {
    async fn read_one(
        &self,
        bus: &dyn BusTransport,
        slave: u8,
        area: RegArea,
        start: u16,
        count: u16,
        timeout: Duration,
    ) -> Result<BlockPayload, DriverError> {
        let fc = area.read_fc();
        let req = frame::build_read_request(slave, fc, start, count);
        let resp = bus.transact(&req, timeout).await?;
        match frame::parse_response(&req, &resp)? {
            RespBody::Words(w) if !area.is_bit_area() => Ok(BlockPayload::Words(w)),
            RespBody::Bits(b) if area.is_bit_area() => Ok(BlockPayload::Bits(b)),
            other => Err(DriverError::Proto(format!(
                "FC{fc:02X} 应答载荷不匹配: {other:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{sim_pair, spawn_slave_endpoint, SimSlave};
    use std::sync::Arc;

    fn blocks(area: RegArea, start: u16, count: u16) -> Vec<ReadBlock> {
        vec![ReadBlock { area, start, count }]
    }

    #[tokio::test]
    async fn read_holding_via_sim() {
        let (bus, endpoint) = sim_pair();
        let slave = Arc::new(SimSlave::new().set_holding(100, 2200).set_holding(101, 15));
        spawn_slave_endpoint(endpoint, slave);

        let d = ModbusRtuDriver::new();
        let r = d
            .read_blocks(
                &bus,
                1,
                &blocks(RegArea::Holding, 100, 2),
                Duration::from_millis(200),
            )
            .await
            .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].payload, BlockPayload::Words(vec![2200, 15]));
    }

    #[tokio::test]
    async fn read_coils_via_sim() {
        let (bus, endpoint) = sim_pair();
        let slave = Arc::new(SimSlave::new().set_coil(3, true).set_coil(5, true));
        spawn_slave_endpoint(endpoint, slave);

        let d = ModbusRtuDriver::new();
        let r = d
            .read_blocks(
                &bus,
                1,
                &blocks(RegArea::Coil, 0, 8),
                Duration::from_millis(200),
            )
            .await
            .unwrap();
        match &r[0].payload {
            BlockPayload::Bits(b) => {
                assert!(b[3] && b[5] && !b[0]);
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn write_registers_single_and_multi() {
        let (bus, endpoint) = sim_pair();
        let slave = Arc::new(SimSlave::new());
        spawn_slave_endpoint(endpoint, slave.clone());

        let d = ModbusRtuDriver::new();
        d.write_registers(&bus, 1, 200, &[42], Duration::from_millis(200))
            .await
            .unwrap();
        assert_eq!(slave.holding_value(200), Some(42));

        d.write_registers(&bus, 1, 300, &[7, 8, 9], Duration::from_millis(200))
            .await
            .unwrap();
        assert_eq!(slave.holding_value(301), Some(8));
    }

    #[tokio::test]
    async fn probe_detects_online_and_offline() {
        let (bus, endpoint) = sim_pair();
        let slave = Arc::new(SimSlave::new());
        spawn_slave_endpoint(endpoint, slave.clone());

        let d = ModbusRtuDriver::new();
        assert!(d.probe(&bus, 1, Duration::from_millis(200)).await.is_ok());

        slave.set_online(false);
        let r = d.probe(&bus, 1, Duration::from_millis(150)).await;
        assert!(matches!(
            r,
            Err(DriverError::Bus(hmi_domain::transport::BusError::Timeout))
        ));
    }

    #[tokio::test]
    async fn oversized_block_split() {
        let (bus, endpoint) = sim_pair();
        let slave = Arc::new(SimSlave::new().set_holding(0, 1));
        spawn_slave_endpoint(endpoint, slave);

        let d = ModbusRtuDriver::new();
        // 300 > 125 → 拆 3 块
        let r = d
            .read_blocks(
                &bus,
                1,
                &blocks(RegArea::Holding, 0, 300),
                Duration::from_millis(200),
            )
            .await
            .unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].start, 0);
        assert_eq!(r[1].start, 125);
        assert_eq!(r[2].start, 250);
        if let BlockPayload::Words(w) = &r[0].payload {
            assert_eq!(w.len(), 125);
        } else {
            panic!()
        }
    }
}
