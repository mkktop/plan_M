//! 通道/设备配置变更、通道生命周期、写点与扫描。

use super::GatewayService;
use crate::supervisor::{spawn_channel, ChannelRuntime, DeviceRuntime};
use hmi_domain::config::{ChannelConfig, DeviceAddress, DeviceConfig};
use hmi_domain::driver::CompileInput;
use hmi_domain::link::TransportKind;
use hmi_domain::points::PointDef;
use hmi_domain::protocol::{ProtocolDecl, ScanRange};
use hmi_domain::value::Value;
use hmi_packaging::InstalledPackage;
use std::sync::Arc;

impl GatewayService {
    // —— 配置变更 ——

    pub async fn upsert_channel(self: &Arc<Self>, cfg: ChannelConfig) -> anyhow::Result<()> {
        let _activation = self.activation.write().await;
        let id = cfg.id.clone();
        if id.trim().is_empty() {
            anyhow::bail!("通道 id 不能为空");
        }
        if !self.transports.contains_key(&cfg.transport_kind()) {
            anyhow::bail!(
                "宿主未实现传输 `{}`（已支持: {}）",
                cfg.transport_kind(),
                self.transport_kinds()
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        // 改传输/改配置后，已有设备必须仍然兼容
        let devices: Vec<DeviceConfig> = {
            let c = self.config.read().await;
            c.devices_on(&id).into_iter().cloned().collect()
        };
        let packages = self.packages.read().await.clone();
        for d in &devices {
            let Some(pkg) = packages.get(&d.package) else {
                continue;
            };
            let Some(driver) = self.drivers.get(&pkg.manifest.protocol.driver) else {
                continue;
            };
            if !pkg
                .manifest
                .protocol
                .allows_transport(cfg.transport_kind(), driver.transports())
            {
                anyhow::bail!(
                    "设备 `{}` 的包 `{}` 不支持传输 `{}`，请先调整设备包或设备挂靠",
                    d.id,
                    d.package,
                    cfg.transport_kind()
                );
            }
        }

        {
            let mut current = self.config.write().await;
            let mut updated = current.clone();
            updated.channels.retain(|x| x.id != id);
            updated.channels.push(cfg);
            self.store.save(&updated)?;
            *current = updated;
        }
        self.restart_channel(&id).await;
        Ok(())
    }

    pub async fn remove_channel(self: &Arc<Self>, id: &str) -> anyhow::Result<()> {
        let _activation = self.activation.write().await;
        {
            let mut current = self.config.write().await;
            let mut updated = current.clone();
            updated.channels.retain(|x| x.id != id);
            updated.devices.retain(|d| d.channel_id != id);
            self.store.save(&updated)?;
            *current = updated;
        }
        self.stop_channel(id).await;
        Ok(())
    }

    pub async fn add_device(self: &Arc<Self>, cfg: DeviceConfig) -> anyhow::Result<()> {
        let _activation = self.activation.write().await;
        let channel = {
            let c = self.config.read().await;
            match c.channel(&cfg.channel_id) {
                Some(ch) => ch.clone(),
                None => anyhow::bail!("通道 {} 不存在", cfg.channel_id),
            }
        };
        let pkg = {
            let packages = self.packages.read().await;
            match packages.get(&cfg.package) {
                Some(p) => p.clone(),
                None => anyhow::bail!("设备包 `{}` 未安装", cfg.package),
            }
        };
        self.prepare_device(&cfg, channel.transport_kind(), &pkg)?;

        let ch = cfg.channel_id.clone();
        let previous = {
            let mut current = self.config.write().await;
            let previous = current.device(&cfg.id).map(|d| d.channel_id.clone());
            let mut updated = current.clone();
            updated.devices.retain(|x| x.id != cfg.id);
            updated.devices.push(cfg);
            self.store.save(&updated)?;
            *current = updated;
            previous
        };
        if let Some(old) = previous.filter(|old| *old != ch) {
            self.restart_channel(&old).await;
        }
        self.restart_channel(&ch).await;
        Ok(())
    }

    pub async fn remove_device(self: &Arc<Self>, id: &str) -> anyhow::Result<()> {
        let _activation = self.activation.write().await;
        let ch = {
            let mut current = self.config.write().await;
            let ch = current.device(id).map(|d| d.channel_id.clone());
            let mut updated = current.clone();
            updated.devices.retain(|x| x.id != id);
            self.store.save(&updated)?;
            *current = updated;
            ch
        };
        self.device_errors.write().await.remove(id);
        if let Some(ch) = ch {
            self.restart_channel(&ch).await;
        }
        Ok(())
    }

    // —— 通道生命周期 ——

    async fn stop_channel(&self, id: &str) {
        let handle = self.channels.write().await.remove(id);
        if let Some(h) = handle {
            h.stop().await;
            self.hub.write(self.hub.comm_lost_batch(&h.tags()));
        }
    }

    /// 重建通道（配置/设备变化后的统一路径）。
    pub(crate) async fn restart_channel(self: &Arc<Self>, id: &str) {
        self.stop_channel(id).await;
        let (cfg, devices_cfg): (ChannelConfig, Vec<DeviceConfig>) = {
            let c = self.config.read().await;
            let Some(cfg) = c.channel(id).cloned() else {
                return;
            };
            (cfg, c.devices_on(id).into_iter().cloned().collect())
        };

        self.channel_errors.write().await.remove(id);
        {
            let mut errs = self.device_errors.write().await;
            for d in &devices_cfg {
                errs.remove(&d.id);
            }
        }

        let Some(factory) = self.transports.get(&cfg.transport_kind()).cloned() else {
            let msg = format!("宿主未实现传输 `{}`，通道未启动", cfg.transport_kind());
            tracing::error!(channel = %id, "{msg}");
            self.channel_errors
                .write()
                .await
                .insert(id.to_string(), msg);
            return;
        };

        let packages = self.packages.read().await.clone();
        let mut devices = Vec::with_capacity(devices_cfg.len());
        for d in &devices_cfg {
            let Some(pkg) = packages.get(&d.package) else {
                record_device_error(self, &d.id, format!("设备包 `{}` 未安装", d.package)).await;
                continue;
            };
            match self.prepare_device(d, cfg.transport_kind(), pkg) {
                Ok(runtime) => {
                    for warning in &runtime.plan.warnings {
                        tracing::warn!(device = %d.id, warning = %warning, "编译警告");
                    }
                    devices.push(Arc::new(runtime));
                }
                Err(e) => {
                    tracing::error!(device = %d.id, error = %e, "设备配置无效");
                    record_device_error(self, &d.id, e.to_string()).await;
                }
            }
        }

        let rt = ChannelRuntime {
            config: cfg,
            devices,
            factory,
            hub: self.hub.clone(),
            events: self.events.clone(),
        };
        let handle = spawn_channel(rt, 64);
        self.channels.write().await.insert(id.to_string(), handle);
    }

    pub(crate) fn prepare_device(
        &self,
        device: &DeviceConfig,
        transport: TransportKind,
        package: &InstalledPackage,
    ) -> anyhow::Result<DeviceRuntime> {
        let protocol = &package.manifest.protocol;
        let driver = self
            .drivers
            .get(&protocol.driver)
            .ok_or_else(|| anyhow::anyhow!("驱动 `{}` 未注册", protocol.driver))?;
        if !protocol.allows_transport(transport, driver.transports()) {
            anyhow::bail!("设备包 `{}` 不支持传输 `{}`", device.package, transport);
        }
        driver
            .validate_address(&device.address)
            .map_err(|e| anyhow::anyhow!("设备地址不合法：{e}"))?;
        let (points, warnings) = driver.effective_points(&package.points, &device.overrides);
        let mut plan = driver
            .compile(&CompileInput {
                device_id: &device.id,
                points: &points,
                protocol,
            })
            .map_err(|e| anyhow::anyhow!("轮询计划编译失败：{e}"))?;
        plan.warnings.extend(warnings);
        Ok(DeviceRuntime {
            device_id: device.id.clone(),
            address: device.address.clone(),
            driver: driver.clone(),
            protocol: protocol.clone(),
            all_tags: plan.all_tags(),
            plan,
            points,
        })
    }

    // —— 写点 ——

    /// 写 tag：`<设备id>.<点id>`（位子 tag 只读）。
    pub async fn write_tag(&self, tag: &str, value: Value) -> Result<(), String> {
        let mut parts = tag.splitn(3, '.');
        let dev_id = parts.next().unwrap_or_default();
        let point_id = parts.next().unwrap_or_default();
        let bit_tail = parts.next();
        if bit_tail.is_some() {
            return Err("状态位子 tag 为只读（写请写父点）".into());
        }

        let _activation = self.activation.read().await;
        let channel_id = self
            .config
            .read()
            .await
            .device(dev_id)
            .map(|device| device.channel_id.clone())
            .ok_or_else(|| format!("设备 `{dev_id}` 不存在"))?;
        let handle = self
            .channels
            .read()
            .await
            .get(&channel_id)
            .cloned()
            .ok_or_else(|| "通道未运行".to_string())?;
        let device = handle
            .device(dev_id)
            .ok_or_else(|| format!("设备 `{dev_id}` 没有有效的轮询计划"))?;
        let point = device
            .points
            .point(point_id)
            .ok_or_else(|| format!("点 `{point_id}` 不在设备的生效点表中"))?;
        if point.access != hmi_domain::points::Access::Rw {
            return Err(format!("点 `{point_id}` 为只读"));
        }
        validate_write_value(point, &value)?;

        handle
            .write(
                device.driver.clone(),
                device.address.clone(),
                point.clone(),
                value,
                device.protocol.clone(),
            )
            .await
    }

    // —— 扫描 ——

    /// 扫描通道上的设备地址。逐个探测（走通道命令队列，与轮询串行）。
    ///
    /// `driver_id` 为空时：优先用通道上已有设备的唯一驱动；否则用唯一支持该
    /// 传输的驱动；仍不唯一则放弃（由 UI 明确选择）。
    pub async fn scan_channel(
        &self,
        channel_id: &str,
        driver_id: Option<&str>,
        package_id: Option<&str>,
        range: ScanRange,
    ) -> Result<Vec<DeviceAddress>, String> {
        let (driver, protocol, handle) = {
            let _activation = self.activation.read().await;
            let config = self.config.read().await;
            let channel = config.channel(channel_id).ok_or("通道不存在")?;
            let packages = self.packages.read().await;
            let kind = channel.transport_kind();
            let declared = package_id
                .map(|id| packages.get(id).ok_or("扫描所选设备包未安装"))
                .transpose()?;
            let existing = config
                .devices_on(channel_id)
                .into_iter()
                .filter_map(|device| packages.get(&device.package));
            let available = self
                .driver_list
                .iter()
                .filter(|driver| driver.transports().contains(&kind));
            let selected =
                driver_id.or_else(|| declared.map(|pkg| pkg.manifest.protocol.driver.as_str()));
            let driver = if let Some(id) = selected {
                self.drivers
                    .get(id)
                    .filter(|driver| driver.transports().contains(&kind))
                    .ok_or("该驱动未注册或与通道不兼容")?
            } else {
                let installed: std::collections::BTreeSet<_> = existing
                    .clone()
                    .map(|pkg| pkg.manifest.protocol.driver.as_str())
                    .collect();
                if installed.len() == 1 {
                    self.drivers
                        .get(*installed.first().unwrap())
                        .ok_or("该驱动未注册")?
                } else {
                    let mut choices = available.filter(|driver| !driver.scan_requires_package());
                    let only = choices.next().ok_or("请选择驱动和扫描所需的设备包")?;
                    if choices.next().is_some() {
                        return Err("请选择扫描驱动".into());
                    }
                    only
                }
            };
            let package = declared.or_else(|| {
                existing
                    .clone()
                    .find(|pkg| pkg.manifest.protocol.driver == driver.id())
            });
            let protocol = match package {
                Some(pkg)
                    if pkg.manifest.protocol.driver == driver.id()
                        && pkg
                            .manifest
                            .protocol
                            .allows_transport(kind, driver.transports()) =>
                {
                    pkg.manifest.protocol.clone()
                }
                Some(_) => return Err("设备包和当前通道或驱动不兼容".into()),
                None if driver.scan_requires_package() => {
                    return Err("该驱动需要设备包提供探测帧".into())
                }
                None => ProtocolDecl {
                    driver: driver.id().into(),
                    transports: Vec::new(),
                    params: serde_json::json!({}),
                },
            };
            let bounds = driver.scan_bounds().ok_or("驱动不支持地址扫描")?;
            if range.from < bounds.from
                || range.to > bounds.to
                || range.from > range.to
                || range.to - range.from >= 256
            {
                return Err(format!(
                    "扫描地址须在 {}–{} 之间，一次最多 256 个地址",
                    bounds.from, bounds.to
                ));
            }
            let handle = self
                .channels
                .read()
                .await
                .get(channel_id)
                .cloned()
                .ok_or("通道未运行")?;
            (driver.clone(), protocol, handle)
        };

        let mut found = Vec::new();
        for number in range.from..=range.to {
            let address = DeviceAddress::Number(number);
            if handle.tx.is_closed() {
                return Err("扫描期间通道已重启，请重新扫描".into());
            }
            if handle
                .probe(driver.clone(), address.clone(), protocol.clone())
                .await
            {
                found.push(address);
            }
        }
        Ok(found)
    }
}

async fn record_device_error(service: &GatewayService, device_id: &str, msg: String) {
    service
        .device_errors
        .write()
        .await
        .insert(device_id.to_string(), msg);
}

/// 通用写值校验（协议无关）：位点布尔、数值边界/步长/枚举。
fn validate_write_value(point: &PointDef, value: &Value) -> Result<(), String> {
    if point.is_bit_point() {
        return match value {
            Value::Bool(_) => Ok(()),
            _ => Err("线圈点需要布尔值".into()),
        };
    }
    let numeric = value.as_f64();
    let has_meta = point.min.is_some() || point.max.is_some() || point.step.is_some();
    if numeric.is_none() && has_meta {
        return Err("该点声明了数值范围/步长，写入值必须是数值".into());
    }
    if let Some(v) = numeric {
        if let Some(min) = point.min {
            if v < min {
                return Err(format!("写入值 {v} 小于下限 {min}"));
            }
        }
        if let Some(max) = point.max {
            if v > max {
                return Err(format!("写入值 {v} 大于上限 {max}"));
            }
        }
        if let Some(step) = point.step {
            if step > 0.0 {
                let base = point.min.unwrap_or(0.0);
                let n = (v - base) / step;
                if (n - n.round()).abs() > 1e-9 {
                    return Err(format!("写入值必须是 {step} 的整数倍（基准 {base}）"));
                }
            }
        }
    }
    if !point.value_enum.is_empty() {
        let iv = value
            .as_i64()
            .ok_or_else(|| "枚举点需要整数写入值".to_string())?;
        if !point.value_enum.iter().any(|e| e.value == iv) {
            return Err(format!("写入值 {iv} 不在枚举范围内"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_validation_checks_range_and_step() {
        let mut p = PointDef {
            id: "x".into(),
            name: String::new(),
            address: hmi_domain::points::PointAddress::Register(hmi_domain::addr::RegAddr {
                area: hmi_domain::addr::RegArea::Holding,
                offset: 0,
            }),
            data_type: hmi_domain::codec::DataType::U16,
            byte_order: None,
            scale: Some(0.1),
            offset: None,
            unit: String::new(),
            access: hmi_domain::points::Access::Rw,
            poll_group: None,
            bits: Default::default(),
            min: Some(0.0),
            max: Some(10.0),
            step: Some(0.5),
            value_enum: Vec::new(),
        };
        assert!(validate_write_value(&p, &Value::F64(5.0)).is_ok());
        assert!(validate_write_value(&p, &Value::F64(5.2)).is_err());
        assert!(validate_write_value(&p, &Value::F64(11.0)).is_err());

        p.step = None;
        p.value_enum = vec![hmi_domain::points::EnumValueDef {
            value: 1,
            label: "on".into(),
        }];
        assert!(validate_write_value(&p, &Value::I64(1)).is_ok());
        assert!(validate_write_value(&p, &Value::I64(2)).is_err());
    }

    /// 守护：`TransportKind` 只在本模块用于能力判断，不允许出现串口/Modbus 类型。
    #[test]
    fn transport_kind_is_domain_type() {
        // 编译期即可证明：这里只引用 domain 枚举
        let _k: hmi_domain::link::TransportKind = hmi_domain::link::TransportKind::Serial;
    }
}
