//! A device package can describe a fixed-length request/reply protocol over a byte link.

use async_trait::async_trait;
use hmi_domain::codec::{apply_scale, decode_registers, encode_num, extract_bit, Num};
use hmi_domain::config::DeviceAddress;
use hmi_domain::driver::{
    CompileInput, DevicePlan, Driver, DriverError, PlanPayload, PollGroup, Sample,
};
use hmi_domain::link::{read_exact, Link, TransportKind};
use hmi_domain::points::{Access, PointAddress, PointDef, PointsTable};
use hmi_domain::protocol::{DriverIssues, ProtocolDecl, ScanRange};
use hmi_domain::value::{TagId, Value};
use serde::Deserialize;
use std::time::{Duration, Instant};

pub struct FrameDriver;

const MAX_FRAME: usize = 512;

#[derive(Clone, Copy)]
enum Byte {
    Literal(u8),
    Unit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FrameSpec {
    request: String,
    response_bytes: usize,
    response_prefix: String,
}

struct Frame {
    request: Vec<Byte>,
    response_bytes: usize,
    prefix: Vec<Byte>,
}

fn parse_bytes(s: &str) -> Result<Vec<Byte>, String> {
    let mut result = Vec::new();
    for part in s.split_ascii_whitespace() {
        let byte = if part == "{unit}" {
            Byte::Unit
        } else {
            Byte::Literal(
                u8::from_str_radix(part, 16)
                    .ok()
                    .filter(|_| part.len() == 2)
                    .ok_or_else(|| format!("`{part}` 不是两位十六进制或 {{unit}}"))?,
            )
        };
        result.push(byte);
        if result.len() > MAX_FRAME {
            return Err(format!("帧最多 {MAX_FRAME} 字节"));
        }
    }
    if result.is_empty() {
        return Err("请求与应答前缀至少需要一个字节".into());
    }
    Ok(result)
}

impl Frame {
    fn new(spec: FrameSpec) -> Result<Self, String> {
        let request = parse_bytes(&spec.request)?;
        let prefix = parse_bytes(&spec.response_prefix)?;
        if !(prefix.len()..=MAX_FRAME).contains(&spec.response_bytes) {
            return Err(format!("应答长度必须覆盖前缀且不超过 {MAX_FRAME} 字节"));
        }
        Ok(Self {
            request,
            prefix,
            response_bytes: spec.response_bytes,
        })
    }

    fn render(bytes: &[Byte], unit: u8) -> Vec<u8> {
        bytes
            .iter()
            .map(|byte| match byte {
                Byte::Literal(n) => *n,
                Byte::Unit => unit,
            })
            .collect()
    }

    async fn exchange(
        &self,
        link: &dyn Link,
        unit: u8,
        timeout: Duration,
        write: Option<(usize, &[u8])>,
    ) -> Result<Vec<u8>, DriverError> {
        let mut request = Self::render(&self.request, unit);
        if let Some((offset, value)) = write {
            request[offset..offset + value.len()].copy_from_slice(value);
        }
        link.send(&request).await?;
        let reply = read_exact(link, self.response_bytes, Instant::now() + timeout).await?;
        if !reply.starts_with(&Self::render(&self.prefix, unit)) {
            return Err(DriverError::Device("应答帧前缀与设备包定义不一致".into()));
        }
        Ok(reply)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AddressSpec {
    read: FrameSpec,
    value_offset: usize,
    write: Option<WriteSpec>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WriteSpec {
    frame: FrameSpec,
    value_offset: usize,
}

struct PointFrame {
    point: PointDef,
    tag: TagId,
    bits: Vec<(u8, TagId)>,
    read: Frame,
    read_offset: usize,
    write: Option<(Frame, usize)>,
}

impl PointFrame {
    fn new(point: &PointDef, device_id: &str) -> Result<Self, String> {
        let PointAddress::Structured(address) = &point.address else {
            return Err("固定帧点位需要对象地址".into());
        };
        let spec: AddressSpec =
            serde_json::from_value(address.clone()).map_err(|error| error.to_string())?;
        let bytes = point.data_type.word_count() * 2;
        let read = Frame::new(spec.read)?;
        if spec.value_offset < read.prefix.len()
            || spec.value_offset.saturating_add(bytes) > read.response_bytes
        {
            return Err("数值字段超出应答范围，或与前缀重叠".into());
        }
        let write = spec
            .write
            .map(|write| -> Result<_, String> {
                let frame = Frame::new(write.frame)?;
                let data = frame
                    .request
                    .get(write.value_offset..write.value_offset.saturating_add(bytes))
                    .ok_or("写入数值字段超出请求范围")?;
                if data.iter().any(|byte| !matches!(byte, Byte::Literal(0))) {
                    return Err("写入数值字段必须在请求帧中用 00 占位".into());
                }
                Ok((frame, write.value_offset))
            })
            .transpose()?;
        if point.access == Access::Rw && write.is_none() {
            return Err("可写点必须声明 write 帧".into());
        }
        Ok(Self {
            point: point.clone(),
            tag: TagId::join(device_id, &point.id),
            bits: point
                .bits
                .keys()
                .map(|&bit| (bit, TagId::join_bit(device_id, &point.id, bit)))
                .collect(),
            read,
            read_offset: spec.value_offset,
            write,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    probe: FrameSpec,
}

fn probe_frame(protocol: &ProtocolDecl) -> Result<Frame, String> {
    Frame::new(protocol.params_as::<Params>()?.probe)
}

fn unit(address: &DeviceAddress) -> Result<u8, String> {
    address
        .as_number()
        .and_then(|n| u8::try_from(n).ok())
        .ok_or_else(|| "固定帧设备地址应为 0–255 的整数".into())
}

#[async_trait]
impl Driver for FrameDriver {
    fn id(&self) -> &'static str {
        "frame"
    }
    fn name(&self) -> &'static str {
        "自定义固定帧"
    }
    fn transports(&self) -> &'static [TransportKind] {
        &[TransportKind::Serial, TransportKind::Tcp]
    }
    fn point_address_format(&self) -> &'static str {
        "frame"
    }
    fn scan_bounds(&self) -> Option<ScanRange> {
        Some(ScanRange { from: 0, to: 255 })
    }
    fn scan_requires_package(&self) -> bool {
        true
    }
    fn validate_address(&self, address: &DeviceAddress) -> Result<(), String> {
        unit(address).map(|_| ())
    }

    fn validate_package(&self, points: &PointsTable, protocol: &ProtocolDecl) -> DriverIssues {
        let mut issues = DriverIssues::default();
        if let Err(error) = probe_frame(protocol) {
            issues.error(format!("探测帧：{error}"));
        }
        for point in &points.points {
            if let Err(error) = PointFrame::new(point, "validation") {
                issues.error(format!("点 {}：{error}", point.id));
            }
        }
        issues
    }

    fn compile(&self, input: &CompileInput<'_>) -> Result<DevicePlan, DriverError> {
        let mut groups = std::collections::BTreeMap::<&str, Vec<PointFrame>>::new();
        for point in &input.points.points {
            groups
                .entry(PointsTable::effective_group(point))
                .or_default()
                .push(PointFrame::new(point, input.device_id).map_err(DriverError::Compile)?);
        }
        Ok(DevicePlan {
            warnings: vec![],
            groups: groups
                .into_iter()
                .map(|(name, points)| {
                    let tags = points
                        .iter()
                        .flat_map(|point| {
                            std::iter::once(point.tag.clone())
                                .chain(point.bits.iter().map(|(_, tag)| tag.clone()))
                        })
                        .collect();
                    PollGroup {
                        name: name.into(),
                        tags,
                        interval: Duration::from_millis(
                            input
                                .points
                                .poll_group(name)
                                .map_or(1000, |p| p.interval_ms),
                        ),
                        payload: PlanPayload::new(points),
                    }
                })
                .collect(),
        })
    }

    async fn poll(
        &self,
        link: &dyn Link,
        address: &DeviceAddress,
        group: &PollGroup,
        timeout: Duration,
    ) -> Result<Vec<Sample>, DriverError> {
        let unit = unit(address).map_err(DriverError::Fatal)?;
        let points = group
            .payload
            .downcast_ref::<Vec<PointFrame>>()
            .ok_or_else(|| DriverError::Fatal("无效的固定帧轮询计划".into()))?;
        let mut values = Vec::with_capacity(group.tags.len());
        for map in points {
            let reply = map.read.exchange(link, unit, timeout, None).await?;
            let end = map.read_offset + map.point.data_type.word_count() * 2;
            let words = reply[map.read_offset..end]
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_be_bytes)
                .collect::<Vec<_>>();
            let num = decode_registers(
                map.point.data_type,
                map.point.byte_order.unwrap_or_default(),
                &words,
            )
            .map_err(|error| DriverError::Device(error.to_string()))?;
            values.push(Sample::good(
                map.tag.clone(),
                apply_scale(num, map.point.scale, map.point.offset),
            ));
            for (bit, tag) in &map.bits {
                values.push(Sample::good(
                    tag.clone(),
                    Value::Bool(extract_bit(words[0], *bit)),
                ));
            }
        }
        Ok(values)
    }

    async fn write(
        &self,
        link: &dyn Link,
        address: &DeviceAddress,
        point: &PointDef,
        value: &Value,
        _protocol: &ProtocolDecl,
        timeout: Duration,
    ) -> Result<(), DriverError> {
        let unit = unit(address).map_err(DriverError::Fatal)?;
        let point_frame = PointFrame::new(point, "write").map_err(DriverError::Compile)?;
        let (frame, offset) = point_frame
            .write
            .ok_or_else(|| DriverError::Unsupported("该点没有写入帧".into()))?;
        let eng = value
            .as_f64()
            .ok_or_else(|| DriverError::Fatal("写入值必须是数值".into()))?;
        let raw = (eng - point.offset.unwrap_or(0.0)) / point.scale.unwrap_or(1.0);
        if !raw.is_finite() {
            return Err(DriverError::Fatal("换算后的写入值无效".into()));
        }
        let codec_value = if point.data_type.is_integer() || point.data_type.is_bcd() {
            if (raw - raw.round()).abs() > 1e-9 || raw < i64::MIN as f64 || raw >= i64::MAX as f64 {
                return Err(DriverError::Fatal("换算后的写入值无法表示为整数".into()));
            }
            Num::I(raw.round() as i64)
        } else {
            Num::F(raw)
        };
        let bytes = encode_num(
            point.data_type,
            point.byte_order.unwrap_or_default(),
            codec_value,
        )
        .map_err(|error| DriverError::Fatal(error.to_string()))?
        .into_iter()
        .flat_map(u16::to_be_bytes)
        .collect::<Vec<_>>();
        frame
            .exchange(link, unit, timeout, Some((offset, &bytes)))
            .await?;
        Ok(())
    }

    async fn probe(
        &self,
        link: &dyn Link,
        address: &DeviceAddress,
        protocol: &ProtocolDecl,
        timeout: Duration,
    ) -> Result<(), DriverError> {
        let unit = unit(address).map_err(DriverError::Fatal)?;
        probe_frame(protocol)
            .map_err(DriverError::Compile)?
            .exchange(link, unit, timeout, None)
            .await?;
        Ok(())
    }
}
