//! GatewayService：把配置存储、包注册表、驱动注册表、通道监督器、TagHub
//! 编成一个可被 CLI/WS 层驱动的整体。

use crate::hub::TagHub;
use crate::poll::compile;
use crate::store::{ConfigStore, JsonConfigStore};
use crate::supervisor::{
    spawn_channel, BusOpener, ChannelHandle, ChannelRuntime, DeviceRuntime, GatewayEvent,
};
use hmi_domain::codec::{encode_num, ByteOrder, Num};
use hmi_domain::config::{DeviceConfig, ProjectConfig, SerialChannelConfig};
use hmi_domain::driver::Driver;
use hmi_domain::value::{Quality, TagChange, TagId, Value};
use hmi_packaging::{install as install_package, PackageRegistry};
use hmi_wire::{ChannelDto, DeviceDto, PackageDto, PointDto, SnapshotData};
use semver::Version;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// 采集服务。一个 gatewayd 进程一个实例。
pub struct GatewayService {
    pub hub: Arc<TagHub>,
    pub host_version: Version,
    pub data_dir: PathBuf,
    drivers: HashMap<String, Arc<dyn Driver>>,
    packages: RwLock<Arc<PackageRegistry>>,
    config: RwLock<ProjectConfig>,
    channels: RwLock<HashMap<String, ChannelHandle>>,
    events: broadcast::Sender<GatewayEvent>,
    store: Box<dyn ConfigStore>,
    open_bus: BusOpener,
}

impl GatewayService {
    /// 组装服务：建目录、读配置、扫包。**不启动通道**（见 [`start`](Self::start)）。
    pub fn new(
        data_dir: impl Into<PathBuf>,
        host_version: Version,
        drivers: Vec<Arc<dyn Driver>>,
        open_bus: BusOpener,
    ) -> anyhow::Result<Arc<Self>> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(data_dir.join("packages"))?;
        std::fs::create_dir_all(data_dir.join("logs"))?;

        let store = Box::new(JsonConfigStore::new(data_dir.join("config.json")));
        let config = store.load()?;
        let registry = PackageRegistry::scan(&data_dir.join("packages"));
        for (path, why) in &registry.broken {
            tracing::warn!(pkg = %path.display(), reason = %why, "跳过损坏的设备包");
        }

        let drivers: HashMap<String, Arc<dyn Driver>> = drivers
            .into_iter()
            .map(|d| (d.id().to_string(), d))
            .collect();

        let (events, _) = broadcast::channel(256);

        Ok(Arc::new(Self {
            hub: Arc::new(TagHub::new()),
            host_version,
            data_dir,
            drivers,
            packages: RwLock::new(Arc::new(registry)),
            config: RwLock::new(config),
            channels: RwLock::new(HashMap::new()),
            events,
            store,
            open_bus,
        }))
    }

    /// 启动配置里的全部通道。
    pub async fn start(self: &Arc<Self>) -> anyhow::Result<()> {
        let ids: Vec<String> = self
            .config
            .read()
            .await
            .channels
            .iter()
            .map(|c| c.id.clone())
            .collect();
        for id in ids {
            self.restart_channel(&id).await;
        }
        Ok(())
    }

    /// 全量快照。
    pub async fn snapshot(&self) -> SnapshotData {
        let cfg = self.config.read().await;
        let channels = self.channels.read().await;
        let packages = self.packages.read().await.clone();

        let channel_dtos = cfg
            .channels
            .iter()
            .map(|c| ChannelDto {
                id: c.id.clone(),
                name: c.name.clone(),
                port: c.port.clone(),
                state: channels
                    .get(&c.id)
                    .map(|h| h.state())
                    .unwrap_or_else(|| "stopped".into()),
            })
            .collect();

        let device_dtos = cfg
            .devices
            .iter()
            .map(|d| DeviceDto {
                id: d.id.clone(),
                name: d.name.clone(),
                channel_id: d.channel_id.clone(),
                package: d.package.clone(),
                slave: d.slave,
                online: device_online_of(&d.id, &d.channel_id, &channels),
            })
            .collect();

        let package_dtos = packages
            .list()
            .iter()
            .map(|p| PackageDto {
                id: p.manifest.id.clone(),
                name: p.manifest.name.clone(),
                vendor: p.manifest.vendor.clone(),
                version: p.manifest.version.to_string(),
                driver: p.manifest.protocol.driver.clone(),
                points: p
                    .points
                    .points
                    .iter()
                    .map(|pt| PointDto {
                        id: pt.id.clone(),
                        name: pt.name.clone(),
                        unit: pt.unit.clone(),
                    })
                    .collect(),
            })
            .collect();

        SnapshotData {
            channels: channel_dtos,
            devices: device_dtos,
            packages: package_dtos,
            tags: self.hub.snapshot_all(),
        }
    }

    /// 订阅事件流（通道状态 / 设备上下线）。
    pub fn subscribe_events(&self) -> broadcast::Receiver<GatewayEvent> {
        self.events.subscribe()
    }

    // —— 配置变更 ——

    pub async fn upsert_channel(self: &Arc<Self>, cfg: SerialChannelConfig) -> anyhow::Result<()> {
        let id = cfg.id.clone();
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
        if !self.config.read().await.channel(&cfg.channel_id).is_some() {
            anyhow::bail!("通道 {} 不存在", cfg.channel_id);
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
        if let Some(ch) = ch {
            self.restart_channel(&ch).await;
        }
        Ok(())
    }

    // —— 通道生命周期 ——

    async fn stop_channel(&self, id: &str) {
        let handle = self.channels.write().await.remove(id);
        if let Some(h) = handle {
            let _ = h.tx.send(crate::supervisor::SupervisorCmd::Stop).await;
        }
    }

    /// 重建通道（配置/设备变化后的统一路径）。
    async fn restart_channel(self: &Arc<Self>, id: &str) {
        self.stop_channel(id).await;
        let (cfg, devices_cfg): (SerialChannelConfig, Vec<DeviceConfig>) = {
            let c = self.config.read().await;
            let Some(cfg) = c.channel(id).cloned() else {
                return;
            };
            (cfg, c.devices_on(id).into_iter().cloned().collect())
        };

        let packages = self.packages.read().await.clone();
        let mut devices = Vec::with_capacity(devices_cfg.len());
        for d in &devices_cfg {
            let Some(pkg) = packages.get(&d.package) else {
                tracing::warn!(device = %d.id, package = %d.package, "设备包未安装，跳过该设备");
                continue;
            };
            let Some(driver) = self.drivers.get(&pkg.manifest.protocol.driver) else {
                tracing::warn!(
                    device = %d.id,
                    driver = %pkg.manifest.protocol.driver,
                    "驱动未注册，跳过该设备"
                );
                continue;
            };
            match compile(
                &d.id,
                &d.overrides,
                &pkg.points,
                &pkg.manifest.protocol.defaults,
            ) {
                Ok(plan) => {
                    for w in &plan.warnings {
                        tracing::warn!(device = %d.id, warning = %w, "编译警告");
                    }
                    let all_tags = plan.groups.iter().flat_map(|g| g.all_tags()).collect();
                    devices.push(DeviceRuntime {
                        device_id: d.id.clone(),
                        slave: d.slave,
                        driver: driver.clone(),
                        plan,
                        all_tags,
                    });
                }
                Err(e) => {
                    tracing::error!(device = %d.id, error = %e, "轮询计划编译失败，跳过该设备");
                }
            }
        }

        // opener：注入的工厂（gatewayd 侧处理真实串口与 SIM 共享总线）
        let opener: BusOpener = self.open_bus.clone();
        let rt = ChannelRuntime {
            cfg,
            devices,
            open_bus: opener,
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

        let (device, point, driver) = {
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
            (device.clone(), point.clone(), driver.clone())
        };
        if point.access != hmi_domain::points::Access::Rw {
            return Err(format!("点 `{point_id}` 为只读"));
        }

        let handle = {
            let channels = self.channels.read().await;
            let Some(h) = channels.get(&device.channel_id) else {
                return Err("通道未运行".into());
            };
            h.clone()
        };

        if point.is_bit_point() {
            let Value::Bool(b) = value else {
                return Err("线圈点需要布尔值".into());
            };
            handle
                .write_coil(driver, device.slave, point.register.offset, b)
                .await
        } else {
            let num = match &value {
                Value::I64(v) => Num::I(*v),
                Value::F64(v) => Num::F(*v),
                _ => return Err("寄存器点需要数值".into()),
            };
            let bo: ByteOrder = point.byte_order.unwrap_or_default();
            let words =
                encode_num(point.data_type, bo, num).map_err(|e| format!("编码写入值失败: {e}"))?;
            handle
                .write_regs(driver, device.slave, point.register.offset, words)
                .await
        }
    }

    // —— 扫描 ——

    /// 扫描通道上的从站。逐个探测（走通道命令队列，与轮询串行）。
    pub async fn scan_channel(&self, channel_id: &str, range: (u8, u8)) -> Vec<u8> {
        let (driver, handle) = {
            let cfg = self.config.read().await;
            let Some(_ch) = cfg.channel(channel_id) else {
                return Vec::new();
            };
            // 扫描用注册表里第一个驱动（多协议总线扫描逐设备驱动，v1 简化）
            let Some(driver) = self.drivers.values().next() else {
                return Vec::new();
            };
            let channels = self.channels.read().await;
            let Some(h) = channels.get(channel_id) else {
                return Vec::new();
            };
            (driver.clone(), h.clone())
        };
        let mut found = Vec::new();
        for slave in range.0..=range.1 {
            if let Ok(true) = handle.probe(driver.clone(), slave).await {
                found.push(slave);
            }
        }
        found
    }

    // —— 包管理 ——

    pub async fn import_package(&self, path: &str) -> Result<PackageDto, String> {
        let installed = install_package(
            std::path::Path::new(path),
            &self.data_dir.join("packages"),
            &self.host_version,
        )
        .map_err(|e| e.to_string())?;
        // 重扫注册表
        let registry = PackageRegistry::scan(&self.data_dir.join("packages"));
        *self.packages.write().await = Arc::new(registry);

        Ok(PackageDto {
            id: installed.manifest.id,
            name: installed.manifest.name,
            vendor: installed.manifest.vendor,
            version: installed.manifest.version.to_string(),
            driver: installed.manifest.protocol.driver,
            points: installed
                .points
                .points
                .iter()
                .map(|p| PointDto {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    unit: p.unit.clone(),
                })
                .collect(),
        })
    }

    pub async fn packages(&self) -> Vec<PackageDto> {
        let packages = self.packages.read().await.clone();
        packages
            .list()
            .iter()
            .map(|p| PackageDto {
                id: p.manifest.id.clone(),
                name: p.manifest.name.clone(),
                vendor: p.manifest.vendor.clone(),
                version: p.manifest.version.to_string(),
                driver: p.manifest.protocol.driver.clone(),
                points: p
                    .points
                    .points
                    .iter()
                    .map(|pt| PointDto {
                        id: pt.id.clone(),
                        name: pt.name.clone(),
                        unit: pt.unit.clone(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// 当前 tag 值（测试/CLI 用）。
    pub fn current_value(&self, tag: &str) -> Option<TagChange> {
        self.hub.get(&TagId::new(tag)).map(|s| TagChange {
            tag: TagId::new(tag),
            value: s.value,
            quality: s.quality,
            ts_ms: s.ts_ms,
        })
    }

    pub fn is_good(&self, tag: &str) -> bool {
        self.hub
            .get(&TagId::new(tag))
            .map(|s| s.quality == Quality::Good)
            .unwrap_or(false)
    }
}

/// 从通道句柄表查设备在线状态（不碰 config 锁，避免与调用方持有的读锁互锁）。
fn device_online_of(
    device_id: &str,
    channel_id: &str,
    channels: &HashMap<String, ChannelHandle>,
) -> bool {
    channels
        .get(channel_id)
        .and_then(|h| {
            h.shared
                .devices_online
                .read()
                .unwrap()
                .get(device_id)
                .map(|f| f.load(std::sync::atomic::Ordering::Relaxed))
        })
        .unwrap_or(false)
}
