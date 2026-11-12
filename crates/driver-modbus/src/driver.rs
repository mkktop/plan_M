//! Modbus 驱动：一个驱动同时支持 RTU 与 MBAP 两种帧封装。
//!
//! 帧封装由设备包 `protocol.params.framing` 决定：
//! - `auto`（默认）：串口/SIM → RTU，TCP → MBAP；
//! - `rtu`：即使走 TCP 也用 RTU（TCP 转 485 网关）；
//! - `tcp`：强制 MBAP。

use hmi_domain::addr::RegArea;
use hmi_domain::codec::{encode_num, DataType, Num};
use hmi_domain::config::DeviceAddress;
use hmi_domain::driver::{CompileInput, DevicePlan, Driver, DriverError, PollGroup, Sample};
use hmi_domain::link::{read_exact, Link, LinkError, TransportKind};
use hmi_domain::points::{Access, PointAddress, PointDef, PointsTable};
use hmi_domain::protocol::{DriverIssues, ProtocolDecl};
use hmi_domain::value::Value;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

use crate::compile::{self, BlockData, BlockPayload, ModbusGroupPlan, ModbusParams, WireFraming};
use crate::{pdu, rtu, tcp};

/// Modbus 驱动（serial / tcp / sim）。
pub struct ModbusDriver {
    next_tid: AtomicU16,
}

impl Default for ModbusDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl ModbusDriver {
    pub fn new() -> Self {
        Self {
            next_tid: AtomicU16::new(1),
        }
    }

    async fn transact(
        &self,
        link: &dyn Link,
        unit: u8,
        req_pdu: &[u8],
        framing: WireFraming,
        timeout: Duration,
    ) -> Result<Vec<u8>, DriverError> {
        match framing {
            WireFraming::Rtu => self.transact_rtu(link, unit, req_pdu, timeout).await,
            WireFraming::Tcp => self.transact_tcp(link, unit, req_pdu, timeout).await,
        }
    }

    async fn transact_rtu(
        &self,
        link: &dyn Link,
        unit: u8,
        req_pdu: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, DriverError> {
        let adu = rtu::build_adu(unit, req_pdu);
        let deadline = Instant::now() + timeout;
        link.send(&adu).await?;

        let header = recv_exact(link, 3, deadline).await?;
        let total = rtu::expected_resp_len(&header).ok_or_else(|| {
            DriverError::Device(format!("无法识别的 RTU 应答帧头: {header:02X?}"))
        })?;
        let rest = recv_exact(link, total.saturating_sub(3), deadline).await?;
        let mut resp = header;
        resp.extend_from_slice(&rest);

        let resp_pdu = rtu::split_adu(unit, &resp)
            .map_err(|e| DriverError::Device(format!("RTU 帧不合法: {e}")))?;
        Ok(resp_pdu.to_vec())
    }

    async fn transact_tcp(
        &self,
        link: &dyn Link,
        unit: u8,
        req_pdu: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, DriverError> {
        let tid = self.next_tid.fetch_add(1, Ordering::Relaxed);
        let frame = tcp::build_frame(tid, unit, req_pdu);
        let deadline = Instant::now() + timeout;
        link.send(&frame).await?;

        let header = recv_exact(link, 6, deadline).await?;
        let (_, len) = tcp::parse_header(&header)
            .map_err(|e| DriverError::Device(format!("MBAP 头不合法: {e}")))?;
        let body = recv_exact(link, len as usize, deadline).await?;
        let mut full = header;
        full.extend_from_slice(&body);

        let (_, _, resp_pdu) = tcp::parse_frame(&full, unit)
            .map_err(|e| DriverError::Device(format!("MBAP 帧不合法: {e}")))?;
        Ok(resp_pdu.to_vec())
    }

    async fn write_pdu(
        &self,
        link: &dyn Link,
        unit: u8,
        req: &[u8],
        framing: WireFraming,
        timeout: Duration,
    ) -> Result<(), DriverError> {
        let resp = self.transact(link, unit, req, framing, timeout).await?;
        match pdu::parse_response(req, &resp) {
            Ok(pdu::RespBody::Echo | pdu::RespBody::WriteOk) => Ok(()),
            Ok(other) => Err(DriverError::Device(format!("写应答类型不匹配: {other:?}"))),
            Err(e) => Err(map_pdu_error(e)),
        }
    }
}

async fn recv_exact(link: &dyn Link, n: usize, deadline: Instant) -> Result<Vec<u8>, DriverError> {
    if n == 0 {
        return Ok(Vec::new());
    }
    read_exact_async(link, n, deadline).await
}

async fn read_exact_async(
    link: &dyn Link,
    n: usize,
    deadline: Instant,
) -> Result<Vec<u8>, DriverError> {
    match read_exact(link, n, deadline).await {
        Ok(v) => Ok(v),
        Err(LinkError::Timeout) => {
            link.drain().await;
            Err(DriverError::Link(LinkError::Timeout))
        }
        Err(e) => Err(DriverError::Link(e)),
    }
}

fn map_pdu_error(e: pdu::PduError) -> DriverError {
    match e {
        pdu::PduError::Exception(fc, code) => {
            let msg = format!("Modbus 异常 fc=0x{fc:02X} code=0x{code:02X}");
            if code == 0x02 {
                DriverError::Fatal(format!("{msg}（非法数据地址）"))
            } else {
                DriverError::Device(msg)
            }
        }
        pdu::PduError::UnsupportedFc(fc) => {
            DriverError::Fatal(format!("不支持的功能码 0x{fc:02X}"))
        }
        other => DriverError::Device(other.to_string()),
    }
}

fn unit_of(address: &DeviceAddress) -> Result<u8, DriverError> {
    let n = address
        .as_number()
        .ok_or_else(|| DriverError::Fatal("Modbus 设备地址必须是数字（1–247）".into()))?;
    if !(1..=247).contains(&n) {
        return Err(DriverError::Fatal(format!(
            "Modbus 从站地址 {n} 超出 1–247"
        )));
    }
    Ok(n as u8)
}

fn params_of(protocol: &ProtocolDecl) -> Result<ModbusParams, DriverError> {
    protocol.params_as().map_err(DriverError::Fatal)
}

fn engineering_to_num(point: &PointDef, value: &Value) -> Result<Num, DriverError> {
    let eng = value
        .as_f64()
        .ok_or_else(|| DriverError::Fatal("寄存器点需要数值".into()))?;
    let scale = point.scale.unwrap_or(1.0);
    if scale == 0.0 {
        return Err(DriverError::Fatal("点表 scale 为 0，无法换算写入值".into()));
    }
    let raw = (eng - point.offset.unwrap_or(0.0)) / scale;
    Ok(
        if matches!(point.data_type, DataType::F32 | DataType::F64) {
            Num::F(raw)
        } else {
            Num::I(raw.round() as i64)
        },
    )
}

#[async_trait::async_trait]
impl Driver for ModbusDriver {
    fn id(&self) -> &'static str {
        "modbus"
    }

    fn name(&self) -> &'static str {
        "Modbus"
    }

    fn transports(&self) -> &'static [TransportKind] {
        &[
            TransportKind::Serial,
            TransportKind::Tcp,
            TransportKind::Sim,
        ]
    }

    fn validate_address(&self, address: &DeviceAddress) -> Result<(), String> {
        let n = address
            .as_number()
            .ok_or_else(|| "Modbus 设备地址必须是数字（1–247）".to_string())?;
        if !(1..=247).contains(&n) {
            return Err(format!("Modbus 从站地址 {n} 超出 1–247"));
        }
        Ok(())
    }

    fn validate_package(&self, points: &PointsTable, protocol: &ProtocolDecl) -> DriverIssues {
        let mut issues = DriverIssues::default();
        let params: ModbusParams = match protocol.params_as() {
            Ok(p) => p,
            Err(e) => {
                issues.error(e);
                return issues;
            }
        };

        for p in &points.points {
            let reg = match &p.address {
                PointAddress::Register(r) => *r,
                PointAddress::Structured(_) => {
                    issues.error(format!(
                        "点 `{}` 使用了非寄存器地址，modbus 驱动不支持",
                        p.id
                    ));
                    continue;
                }
            };

            if p.access == Access::Rw && !reg.area.writable() {
                issues.error(format!(
                    "点 `{}` 位于只读区域 {:?}，不能声明 access=rw",
                    p.id, reg.area
                ));
            }

            if reg.area.is_bit_area() {
                if !p.bits.is_empty() {
                    issues.error(format!("位区域点 `{}` 不能声明 bits 展开", p.id));
                }
                if p.min.is_some()
                    || p.max.is_some()
                    || p.step.is_some()
                    || !p.value_enum.is_empty()
                {
                    issues.error(format!("位区域点 `{}` 不能声明 min/max/step/enum", p.id));
                }
                continue;
            }

            let wc = p.data_type.word_count() as u32;
            if reg.offset as u32 + wc > 65536 {
                issues.error(format!(
                    "点 `{}` 超出 16 位地址空间（offset {} + {} 字）",
                    p.id, reg.offset, wc
                ));
            }
            if !p.bits.is_empty() {
                if wc != 1 {
                    issues.error(format!(
                        "点 `{}` 为多字类型，bits 展开只支持单字（16 位）",
                        p.id
                    ));
                }
                for (&bit, def) in &p.bits {
                    if bit >= 16 {
                        issues.error(format!("点 `{}` 的位号 {bit} 超出 0–15", p.id));
                    }
                    if def.name.trim().is_empty() {
                        issues.error(format!("点 `{}` 的位 {bit} 缺少名称", p.id));
                    }
                }
            }
            if wc > 1 && p.byte_order.is_none() && params.byte_order.is_none() {
                issues.warning(format!(
                    "点 `{}` 为多字类型但未声明 byteOrder（将按 abcd 解析）",
                    p.id
                ));
            }
            if p.scale == Some(0.0) {
                issues.error(format!("点 `{}` 的 scale 不能为 0", p.id));
            }
        }

        issues
    }

    fn compile(&self, input: &CompileInput<'_>) -> Result<DevicePlan, DriverError> {
        compile::compile(input)
    }

    async fn poll(
        &self,
        link: &dyn Link,
        address: &DeviceAddress,
        group: &PollGroup,
        timeout: Duration,
    ) -> Result<Vec<Sample>, DriverError> {
        let unit = unit_of(address)?;
        let plan = group
            .payload
            .downcast_ref::<ModbusGroupPlan>()
            .ok_or_else(|| DriverError::Fatal("Modbus 轮询计划类型不匹配".into()))?;
        let framing = plan.framing.resolve(link.kind());

        let mut data = Vec::with_capacity(plan.blocks.len());
        for block in &plan.blocks {
            let fc = block.area.read_fc();
            let req = pdu::build_read(fc, block.start, block.count);
            let resp = self.transact(link, unit, &req, framing, timeout).await?;
            let body = pdu::parse_response(&req, &resp).map_err(map_pdu_error)?;
            let payload = match body {
                pdu::RespBody::Words(w) if !block.area.is_bit_area() => BlockPayload::Words(w),
                pdu::RespBody::Bits(b) if block.area.is_bit_area() => BlockPayload::Bits(b),
                other => {
                    return Err(DriverError::Device(format!(
                        "FC{fc:02X} 应答载荷不匹配: {other:?}"
                    )))
                }
            };
            data.push(BlockData {
                area: block.area,
                start: block.start,
                payload,
            });
        }
        Ok(compile::decode(plan, &data))
    }

    async fn write(
        &self,
        link: &dyn Link,
        address: &DeviceAddress,
        point: &PointDef,
        value: &Value,
        protocol: &ProtocolDecl,
        timeout: Duration,
    ) -> Result<(), DriverError> {
        let unit = unit_of(address)?;
        let params = params_of(protocol)?;
        let reg = match &point.address {
            PointAddress::Register(r) => *r,
            PointAddress::Structured(_) => {
                return Err(DriverError::Fatal("modbus 驱动不支持非寄存器地址".into()))
            }
        };
        let framing = params.framing.resolve(link.kind());

        match reg.area {
            RegArea::Coil => {
                let on = value
                    .as_bool()
                    .ok_or_else(|| DriverError::Fatal("线圈点需要布尔值".into()))?;
                let req = pdu::build_write_single_coil(reg.offset, on);
                self.write_pdu(link, unit, &req, framing, timeout).await
            }
            RegArea::Holding => {
                let raw = engineering_to_num(point, value)?;
                let bo = point.byte_order.or(params.byte_order).unwrap_or_default();
                let words = encode_num(point.data_type, bo, raw)
                    .map_err(|e| DriverError::Fatal(format!("写值编码失败: {e}")))?;
                if words.len() > 123 {
                    return Err(DriverError::Fatal("单次写寄存器不能超过 123 个".into()));
                }
                let req = if words.len() == 1 {
                    pdu::build_write_single_reg(reg.offset, words[0])
                } else {
                    pdu::build_write_multi_regs(reg.offset, &words)
                };
                self.write_pdu(link, unit, &req, framing, timeout).await
            }
            RegArea::Input | RegArea::Discrete => {
                Err(DriverError::Fatal("该区域只读，不能写入".into()))
            }
        }
    }

    async fn probe(
        &self,
        link: &dyn Link,
        address: &DeviceAddress,
        protocol: &ProtocolDecl,
        timeout: Duration,
    ) -> Result<(), DriverError> {
        let unit = unit_of(address)?;
        let params = params_of(protocol)?;
        let framing = params.framing.resolve(link.kind());
        let req = pdu::build_read(pdu::FC_READ_HOLDING, 0, 1);
        let resp = self.transact(link, unit, &req, framing, timeout).await?;
        match pdu::parse_response(&req, &resp) {
            Ok(_) => Ok(()),
            // 有应答（哪怕是异常码）就说明设备在线
            Err(pdu::PduError::Exception(_, _)) => Ok(()),
            Err(e) => Err(map_pdu_error(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmi_domain::points::{PointsTable, PollGroupDef};
    use hmi_domain::protocol::ProtocolDecl;
    use hmi_transport_sim::sim_pair;
    use std::sync::Arc;

    fn decl(params: serde_json::Value) -> ProtocolDecl {
        ProtocolDecl {
            driver: "modbus".into(),
            transports: Vec::new(),
            params,
        }
    }

    fn table(points: Vec<PointDef>) -> PointsTable {
        PointsTable {
            poll_groups: vec![PollGroupDef {
                name: "default".into(),
                interval_ms: 1000,
            }],
            points,
        }
    }

    fn holding_point(id: &str, off: u16) -> PointDef {
        PointDef {
            id: id.into(),
            name: String::new(),
            address: PointAddress::Register(hmi_domain::addr::RegAddr {
                area: RegArea::Holding,
                offset: off,
            }),
            data_type: hmi_domain::codec::DataType::U16,
            byte_order: None,
            scale: None,
            offset: None,
            unit: String::new(),
            access: Access::Rw,
            poll_group: None,
            bits: Default::default(),
            min: None,
            max: None,
            step: None,
            value_enum: Vec::new(),
        }
    }

    fn compile_for(driver: &ModbusDriver, points: PointsTable, d: &ProtocolDecl) -> DevicePlan {
        driver
            .compile(&CompileInput {
                device_id: "dev1",
                overrides: &Default::default(),
                points: &points,
                protocol: d,
            })
            .unwrap()
    }

    #[tokio::test]
    async fn rtu_read_and_write_via_sim() {
        let (link, endpoint) = sim_pair();
        let slave = Arc::new(
            crate::sim::SimSlave::new()
                .set_holding(100, 2200)
                .set_holding(101, 15),
        );
        let responder = crate::sim::rtu_responder(slave.clone());
        tokio::spawn(endpoint.run(move |req| responder(req)));

        let driver = ModbusDriver::new();
        let d = decl(serde_json::json!({}));
        let points = table(vec![holding_point("v", 100), holding_point("i", 101)]);
        let plan = compile_for(&driver, points, &d);

        let samples = driver
            .poll(
                &link,
                &1.into(),
                &plan.groups[0],
                Duration::from_millis(200),
            )
            .await
            .unwrap();
        let map: std::collections::HashMap<_, _> = samples
            .into_iter()
            .map(|s| (s.tag.to_string(), s.value))
            .collect();
        assert_eq!(map.get("dev1.v"), Some(&Value::I64(2200)));
        assert_eq!(map.get("dev1.i"), Some(&Value::I64(15)));

        driver
            .write(
                &link,
                &1.into(),
                &holding_point("v", 100),
                &Value::I64(5678),
                &d,
                Duration::from_millis(200),
            )
            .await
            .unwrap();
        assert_eq!(slave.holding_value(100), Some(5678));
    }

    #[tokio::test]
    async fn probe_online_and_offline() {
        let (link, endpoint) = sim_pair();
        let slave = Arc::new(crate::sim::SimSlave::new());
        let responder = crate::sim::rtu_responder(slave.clone());
        tokio::spawn(endpoint.run(move |req| responder(req)));

        let driver = ModbusDriver::new();
        let d = decl(serde_json::json!({}));
        assert!(driver
            .probe(&link, &1.into(), &d, Duration::from_millis(200))
            .await
            .is_ok());

        slave.set_online(false);
        let err = driver
            .probe(&link, &1.into(), &d, Duration::from_millis(80))
            .await
            .unwrap_err();
        assert_eq!(err.poll_class(), hmi_domain::driver::PollClass::Retry);
    }

    #[tokio::test]
    async fn tcp_read_via_loopback() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let slave = Arc::new(crate::sim::SimSlave::new().set_holding(100, 2200));
        let slave2 = slave.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _ = crate::sim::serve_tcp_stream(stream, slave2).await;
        });

        let params = hmi_domain::config::TcpParams {
            host: "127.0.0.1".into(),
            port: addr.port(),
            connect_timeout_ms: 1000,
        };
        let link = hmi_transport_tcp::TcpLink::connect(&params).await.unwrap();

        let driver = ModbusDriver::new();
        let d = decl(serde_json::json!({}));
        let plan = compile_for(&driver, table(vec![holding_point("v", 100)]), &d);
        let samples = driver
            .poll(
                &link,
                &1.into(),
                &plan.groups[0],
                Duration::from_millis(300),
            )
            .await
            .unwrap();
        assert_eq!(samples[0].value, Value::I64(2200));
    }

    #[test]
    fn address_validation() {
        let d = ModbusDriver::new();
        assert!(d.validate_address(&1.into()).is_ok());
        assert!(d.validate_address(&0.into()).is_err());
        assert!(d.validate_address(&248.into()).is_err());
        assert!(d.validate_address(&"AA:BB".into()).is_err());
    }

    #[test]
    fn package_validation_rejects_structured_points() {
        let points: PointsTable = serde_json::from_value(serde_json::json!({
            "points": [{ "id": "x", "address": { "ble": {} } }]
        }))
        .unwrap();
        let issues = ModbusDriver::new().validate_package(&points, &decl(serde_json::json!({})));
        assert!(!issues.errors.is_empty());
    }
}
