//! 通道/设备配置变更、通道生命周期、写点与扫描。

use super::GatewayService;
use crate::supervisor::{spawn_channel, ChannelRuntime, DeviceRuntime};
use hmi_domain::config::{ChannelConfig, DeviceAddress, DeviceConfig};
use hmi_domain::driver::CompileInput;
use hmi_domain::points::PointDef;
use hmi_domain::protocol::{ProtocolDecl, ScanRange};
use hmi_domain::value::Value;
use std::sync::Arc;

impl GatewayService {
    // —— 配置变更 ——

    pub async fn upsert_channel(self: &Arc<Self>, cfg: ChannelConfig) -> anyhow::Result<()> {
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

        self.stop_channel(&id).await;
        {
            let mut c = self.config.write().await;
            c.channels.retain(|x| x.id != id);
            c.channels.push(cfg);
            self.store.save(&c)?;
        }
        self.restart_channel(&id).await;
        Ok(())
    }

    pub async fn remove_channel(self: &Arc<Self>, id: &str) -> anyhow::Result<()> {
        self.stop_channel(id).await;
        let mut c = self.config.write().await;
        c.channels.retain(|x| x.id != id);
        // 挂在该通道下的设备一并移除
        c.devices.retain(|d| d.channel_id != id);
        self.store.save(&c)?;
        Ok(())
    }

    pub async fn add_device(self: &Arc<Self>, cfg: DeviceConfig) -> anyhow::Result<()> {
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
        let driver = self
            .drivers
            .get(&pkg.manifest.protocol.driver)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("驱动 `{}` 未注册", pkg.manifest.protocol.driver))?;

        if !driver.transports().contains(&channel.transport_kind()) {
            anyhow::bail!(
                "驱动 `{}` 不支持通道传输 `{}`",
                driver.id(),
                channel.transport_kind()
            );
        }
        if !pkg
            .manifest
            .protocol
            .allows_transport(channel.transport_kind(), driver.transports())
        {
            anyhow::bail!(
                "设备包 `{}` 不支持通道传输 `{}`",
                cfg.package,
                channel.transport_kind()
            );
        }
        if let Err(e) = driver.validate_address(&cfg.address) {
            anyhow::bail!("设备地址不合法：{e}");
        }

        let ch = cfg.channel_id.clone();
        {
            let mut c = self.config.write().await;
            c.devices.retain(|x| x.id != cfg.id);
            c.devices.push(cfg);
            self.store.save(&c)?;
        }
        self.restart_channel(&ch).await;
        Ok(())
    }

    pub async fn remove_device(self: &Arc<Self>, id: &str) -> anyhow::Result<()> {
        let ch = {
            let mut c = self.config.write().await;
            let ch = c.device(id).map(|d| d.channel_id.clone());
            c.devices.retain(|x| x.id != id);
            self.store.save(&c)?;
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
            let Some(driver) = self.drivers.get(&pkg.manifest.protocol.driver).cloned() else {
                record_device_error(
                    self,
                    &d.id,
                    format!("驱动 `{}` 未注册", pkg.manifest.protocol.driver),
                )
                .await;
                continue;
            };
            if !driver.transports().contains(&cfg.transport_kind()) {
                record_device_error(
                    self,
                    &d.id,
                    format!(
                        "驱动 `{}` 不支持传输 `{}`",
                        driver.id(),
                        cfg.transport_kind()
                    ),
                )
                .await;
                continue;
            }
            if !pkg
                .manifest
                .protocol
                .allows_transport(cfg.transport_kind(), driver.transports())
            {
                record_device_error(
                    self,
                    &d.id,
                    format!(
                        "设备包 `{}` 不支持传输 `{}`",
                        d.package,
                        cfg.transport_kind()
                    ),
                )
                .await;
                continue;
            }
            let input = CompileInput {
                device_id: &d.id,
                overrides: &d.overrides,
                points: &pkg.points,
                protocol: &pkg.manifest.protocol,
            };
            match driver.compile(&input) {
                Ok(plan) => {
                    for w in &plan.warnings {
                        tracing::warn!(device = %d.id, warning = %w, "编译警告");
                    }
                    devices.push(DeviceRuntime {
                        device_id: d.id.clone(),
                        address: d.address.clone(),
                        driver: driver.clone(),
                        protocol: pkg.manifest.protocol.clone(),
                        all_tags: plan.all_tags(),
                        plan,
                    });
                }
                Err(e) => {
                    tracing::error!(device = %d.id, error = %e, "轮询计划编译失败，跳过该设备");
                    record_device_error(self, &d.id, format!("轮询计划编译失败：{e}")).await;
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

        let (device, point, driver, protocol) = {
            let cfg = self.config.read().await;
            let Some(device) = cfg.device(dev_id) else {
                return Err(format!("设备 `{dev_id}` 不存在"));
            };
            let packages = self.packages.read().await;
            let Some(pkg) = packages.get(&device.package) else {
                return Err(format!("设备包 `{}` 未安装", device.package));
            };
            let Some(point) = pkg.points.point(point_id) else {
                return Err(format!(
                    "点 `{point_id}` 不在包 {} 的点表中",
                    device.package
                ));
            };
            let Some(driver) = self.drivers.get(&pkg.manifest.protocol.driver) else {
                return Err("驱动未注册".into());
            };
            (
                device.clone(),
                point.clone(),
                driver.clone(),
                pkg.manifest.protocol.clone(),
            )
        };
        if point.access != hmi_domain::points::Access::Rw {
            return Err(format!("点 `{point_id}` 为只读"));
        }
        validate_write_value(&point, &value)?;

        let handle = {
            let channels = self.channels.read().await;
            let Some(h) = channels.get(&device.channel_id) else {
                return Err("通道未运行".into());
            };
            h.clone()
        };

        handle
            .write(driver, device.address.clone(), point, value, protocol)
            .await
    }

    // —— 扫描 ——

    /// 扫描通道上的设备地址。逐个探测（走通道命令队列，与轮询串行）。
    ///
    /// `driver_id` 为空时：优先用通道上已有设备的唯一驱动；否则用唯一支持该
    /// 传输的驱动；仍不唯一则放弃（由 UI 明确选择）。
    pub async fn scan_channel<F>(
        &self,
        channel_id: &str,
        driver_id: Option<&str>,
        range: ScanRange,
        mut progress: F,
    ) -> Vec<DeviceAddress>
    where
        F: FnMut(DeviceAddress, bool) + Send,
    {
        let (channel, devices) = {
            let cfg = self.config.read().await;
            let Some(ch) = cfg.channel(channel_id) else {
                return Vec::new();
            };
            (
                ch.clone(),
                cfg.devices_on(channel_id)
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        };
        let packages = self.packages.read().await.clone();

        let driver = match driver_id {
            Some(id) => self.drivers.get(id).cloned(),
            None => {
                let mut ids: Vec<String> = devices
                    .iter()
                    .filter_map(|d| packages.get(&d.package))
                    .map(|p| p.manifest.protocol.driver.clone())
                    .collect();
                ids.sort();
                ids.dedup();
                if ids.len() == 1 {
                    self.drivers.get(&ids[0]).cloned()
                } else {
                    let candidates: Vec<Arc<dyn hmi_domain::driver::Driver>> = self
                        .driver_list
                        .iter()
                        .filter(|d| d.transports().contains(&channel.transport_kind()))
                        .cloned()
                        .collect();
                    if candidates.len() == 1 {
                        Some(candidates[0].clone())
                    } else {
                        None
                    }
                }
            }
        };
        let Some(driver) = driver else {
            return Vec::new();
        };
        if !driver.transports().contains(&channel.transport_kind()) {
            return Vec::new();
        }

        // 扫描时优先用同通道已有设备的包参数（framing 等）
        let protocol = devices
            .iter()
            .filter_map(|d| packages.get(&d.package))
            .find(|p| p.manifest.protocol.driver == driver.id())
            .map(|p| p.manifest.protocol.clone())
            .unwrap_or_else(|| ProtocolDecl {
                driver: driver.id().to_string(),
                transports: Vec::new(),
                params: serde_json::json!({}),
            });

        let handle = {
            let channels = self.channels.read().await;
            let Some(h) = channels.get(channel_id) else {
                return Vec::new();
            };
            h.clone()
        };

        let mut found = Vec::new();
        for n in range.from..=range.to {
            let addr = DeviceAddress::Number(n);
            let ok = handle
                .probe(driver.clone(), addr.clone(), protocol.clone())
                .await;
            progress(addr.clone(), ok);
            if ok {
                found.push(addr);
            }
        }
        found
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
