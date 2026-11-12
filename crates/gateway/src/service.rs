//! GatewayService：把配置存储、包注册表、驱动/传输注册表、通道监督器、TagHub
//! 编成一个可被 CLI/WS 层驱动的整体。

use crate::dashboards::{dashboards_path, DashboardStore};
use crate::hub::TagHub;
use crate::store::{ConfigStore, JsonConfigStore};
use crate::supervisor::{
    spawn_channel, ChannelHandle, ChannelRuntime, DeviceRuntime, GatewayEvent,
};
use crate::workspaces::{workspaces_path, WorkspaceStore};
use hmi_domain::config::{ChannelConfig, DeviceAddress, DeviceConfig, ProjectConfig};
use hmi_domain::driver::{CompileInput, Driver};
use hmi_domain::link::{TransportFactory, TransportKind, TransportTarget};
use hmi_domain::points::{PointDef, PointsTable};
use hmi_domain::protocol::{ProtocolDecl, ScanRange};
use hmi_domain::value::{Quality, TagChange, TagId, Value};
use hmi_packaging::{
    install as install_package, install_ui_plugin, BindingSpec as PkgBindingSpec, InstalledPackage,
    InstalledUiPlugin, PackageManifest, PackageRegistry, UiPluginRegistry, ValidationIssues,
};
use hmi_wire::{
    BindingDeclDto, BindingSpecDto, ChannelDto, DashboardDto, DeviceDto, DevicePageComponentDto,
    DevicePageDto, DriverInfoDto, PackageDto, PackageUiDto, PackageWidgetDto, PointDto,
    SnapshotData, UiDevicePanelDto, UiPageDto, UiPluginDto, UiWidgetDto, WidgetSizeDto,
    WorkspaceDto,
};
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
    driver_list: Vec<Arc<dyn Driver>>,
    transports: HashMap<TransportKind, Arc<dyn TransportFactory>>,
    packages: RwLock<Arc<PackageRegistry>>,
    ui_plugins: RwLock<UiPluginRegistry>,
    config: RwLock<ProjectConfig>,
    dashboards: RwLock<DashboardStore>,
    workspaces: RwLock<WorkspaceStore>,
    channels: RwLock<HashMap<String, ChannelHandle>>,
    /// 通道配置错误（传输未注册等）
    channel_errors: RwLock<HashMap<String, String>>,
    /// 设备配置错误（驱动缺失、编译失败、传输不兼容等）
    device_errors: RwLock<HashMap<String, String>>,
    events: broadcast::Sender<GatewayEvent>,
    store: Box<dyn ConfigStore>,
}

impl GatewayService {
    /// 组装服务：建目录、读配置、扫包。**不启动通道**（见 [`start`](Self::start)）。
    pub fn new(
        data_dir: impl Into<PathBuf>,
        host_version: Version,
        drivers: Vec<Arc<dyn Driver>>,
        transports: Vec<Arc<dyn TransportFactory>>,
    ) -> anyhow::Result<Arc<Self>> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(data_dir.join("packages"))?;
        std::fs::create_dir_all(data_dir.join("ui-plugins"))?;
        std::fs::create_dir_all(data_dir.join("logs"))?;

        let store = Box::new(JsonConfigStore::new(data_dir.join("config.json")));
        let config = store.load()?;
        let dashboards = DashboardStore::load(dashboards_path(&data_dir));
        let workspaces = WorkspaceStore::load(workspaces_path(&data_dir));

        let driver_list: Vec<Arc<dyn Driver>> = drivers;
        let drivers: HashMap<String, Arc<dyn Driver>> = driver_list
            .iter()
            .map(|d| (d.id().to_string(), d.clone()))
            .collect();
        let transports: HashMap<TransportKind, Arc<dyn TransportFactory>> =
            transports.into_iter().map(|t| (t.kind(), t)).collect();

        let registry = PackageRegistry::scan(&data_dir.join("packages"), &driver_list);
        for (path, why) in &registry.broken {
            tracing::warn!(pkg = %path.display(), reason = %why, "跳过损坏的设备包");
        }
        let ui_plugins = UiPluginRegistry::scan(&data_dir.join("ui-plugins"));
        for (path, why) in &ui_plugins.broken {
            tracing::warn!(plugin = %path.display(), reason = %why, "跳过损坏的 UI 插件");
        }

        let (events, _) = broadcast::channel(256);

        Ok(Arc::new(Self {
            hub: Arc::new(TagHub::new()),
            host_version,
            data_dir,
            drivers,
            driver_list,
            transports,
            packages: RwLock::new(Arc::new(registry)),
            ui_plugins: RwLock::new(ui_plugins),
            config: RwLock::new(config),
            dashboards: RwLock::new(dashboards),
            workspaces: RwLock::new(workspaces),
            channels: RwLock::new(HashMap::new()),
            channel_errors: RwLock::new(HashMap::new()),
            device_errors: RwLock::new(HashMap::new()),
            events,
            store,
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

    /// 宿主支持的传输种类（Welcome 能力协商）。
    pub fn transport_kinds(&self) -> Vec<TransportKind> {
        let mut kinds: Vec<TransportKind> = self.transports.keys().copied().collect();
        kinds.sort_by_key(|k| k.as_str());
        kinds
    }

    /// 宿主注册的驱动能力。
    pub fn driver_info(&self) -> Vec<DriverInfoDto> {
        let mut list: Vec<DriverInfoDto> = self
            .driver_list
            .iter()
            .map(|d| DriverInfoDto {
                id: d.id().to_string(),
                name: d.name().to_string(),
                transports: d.transports().to_vec(),
            })
            .collect();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        list
    }

    /// 某种传输的可发现目标（串口列表等）。
    pub async fn transport_targets(&self, kind: TransportKind) -> Vec<TransportTarget> {
        match self.transports.get(&kind) {
            Some(f) => f.targets().await,
            None => Vec::new(),
        }
    }

    /// 全量快照。
    pub async fn snapshot(&self) -> SnapshotData {
        let cfg = self.config.read().await;
        let channels = self.channels.read().await;
        let packages = self.packages.read().await.clone();
        let channel_errors = self.channel_errors.read().await;
        let device_errors = self.device_errors.read().await;

        let channel_dtos = cfg
            .channels
            .iter()
            .map(|c| ChannelDto {
                config: c.clone(),
                state: channels
                    .get(&c.id)
                    .map(|h| h.state())
                    .unwrap_or_else(|| "stopped".into()),
                config_error: channel_errors.get(&c.id).cloned(),
            })
            .collect();

        let device_dtos = cfg
            .devices
            .iter()
            .map(|d| DeviceDto {
                config: d.clone(),
                online: device_online_of(&d.id, &d.channel_id, &channels),
                config_error: device_errors.get(&d.id).cloned(),
            })
            .collect();

        let package_dtos = packages.list().iter().map(package_dto).collect();

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
    async fn restart_channel(self: &Arc<Self>, id: &str) {
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
                    let candidates: Vec<Arc<dyn Driver>> = self
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

    // —— 包管理 ——

    pub async fn import_package(&self, path: &str) -> Result<PackageDto, String> {
        let installed = install_package(
            std::path::Path::new(path),
            &self.data_dir.join("packages"),
            &self.host_version,
            &self.driver_list,
        )
        .map_err(|e| e.to_string())?;
        self.reload_packages().await;
        Ok(package_dto(&installed))
    }

    pub async fn packages(&self) -> Vec<PackageDto> {
        let packages = self.packages.read().await.clone();
        packages.list().iter().map(package_dto).collect()
    }

    /// 删除设备包（该 id 的全部版本目录）；有设备在用时拒绝，未安装报错。
    pub async fn remove_package(&self, id: &str) -> Result<(), String> {
        if !valid_id(id) {
            return Err(format!("设备包 id 非法：{id}"));
        }
        let in_use = self
            .config
            .read()
            .await
            .devices
            .iter()
            .filter(|d| d.package == id)
            .count();
        if in_use > 0 {
            return Err(format!(
                "该设备包正被 {in_use} 台设备使用，请先删除这些设备再移除"
            ));
        }
        {
            let registry = self.packages.read().await;
            if registry.get(id).is_none() {
                return Err(format!("设备包 `{id}` 未安装"));
            }
        }
        let dir = self.data_dir.join("packages").join(id);
        std::fs::remove_dir_all(&dir).map_err(|e| format!("删除设备包目录失败：{e}"))?;
        self.reload_packages().await;
        Ok(())
    }

    async fn reload_packages(&self) {
        let registry = PackageRegistry::scan(&self.data_dir.join("packages"), &self.driver_list);
        for (path, why) in &registry.broken {
            tracing::warn!(pkg = %path.display(), reason = %why, "跳过损坏的设备包");
        }
        *self.packages.write().await = Arc::new(registry);
    }

    // —— 包编辑器：草稿校验 / 安装 / 导出（P4）——

    /// 校验 UI 生成的包草稿（不落盘）。
    pub fn validate_package_draft(
        &self,
        manifest_json: &serde_json::Value,
        points_json: &serde_json::Value,
    ) -> Result<ValidationIssues, String> {
        let (manifest, points) = parse_package_draft(manifest_json, points_json)?;
        Ok(hmi_packaging::validate_package(
            &manifest,
            &points,
            &self.driver_list,
        ))
    }

    /// 安装 UI 生成的包草稿：写到临时目录后走正式安装流（同一套校验/落盘规则）。
    pub async fn install_draft_package(
        &self,
        manifest_json: &serde_json::Value,
        points_json: &serde_json::Value,
    ) -> Result<PackageDto, String> {
        let (manifest, points) = parse_package_draft(manifest_json, points_json)?;
        let issues = hmi_packaging::validate_package(&manifest, &points, &self.driver_list);
        if !issues.ok() {
            return Err(format!(
                "包校验未通过：\n  - {}",
                issues.errors.join("\n  - ")
            ));
        }
        let dir = std::env::temp_dir().join(format!(
            "hmi-draft-pkg-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败：{e}"))?;
        let written = (|| -> Result<(), String> {
            let m = serde_json::to_string_pretty(manifest_json)
                .map_err(|e| format!("序列化 manifest 失败：{e}"))?;
            let p = serde_json::to_string_pretty(points_json)
                .map_err(|e| format!("序列化点表失败：{e}"))?;
            std::fs::write(dir.join("manifest.json"), m)
                .map_err(|e| format!("写入 manifest 失败：{e}"))?;
            std::fs::write(dir.join("points.json"), p).map_err(|e| format!("写入点表失败：{e}"))?;
            Ok(())
        })();
        let result = written.and_then(|()| {
            install_package(
                &dir,
                &self.data_dir.join("packages"),
                &self.host_version,
                &self.driver_list,
            )
            .map_err(|e| e.to_string())
        });
        let _ = std::fs::remove_dir_all(&dir);
        let installed = result?;
        self.reload_packages().await;
        Ok(package_dto(&installed))
    }

    /// 导出包草稿到本机目录（写 manifest.json + points.json，保留原始 JSON 形态）。
    pub fn export_package_draft(
        &self,
        manifest_json: &serde_json::Value,
        points_json: &serde_json::Value,
        dir: &str,
    ) -> Result<(), String> {
        if dir.trim().is_empty() {
            return Err("请填写导出目录".into());
        }
        let (manifest, points) = parse_package_draft(manifest_json, points_json)?;
        let issues = hmi_packaging::validate_package(&manifest, &points, &self.driver_list);
        if !issues.ok() {
            return Err(format!(
                "包校验未通过：\n  - {}",
                issues.errors.join("\n  - ")
            ));
        }
        let dir = std::path::Path::new(dir);
        if dir.exists() && !dir.is_dir() {
            return Err("导出路径不是目录".into());
        }
        std::fs::create_dir_all(dir).map_err(|e| format!("创建导出目录失败：{e}"))?;
        let m = serde_json::to_string_pretty(manifest_json)
            .map_err(|e| format!("序列化 manifest 失败：{e}"))?;
        let p = serde_json::to_string_pretty(points_json)
            .map_err(|e| format!("序列化点表失败：{e}"))?;
        std::fs::write(dir.join("manifest.json"), m)
            .map_err(|e| format!("写入 manifest 失败：{e}"))?;
        std::fs::write(dir.join("points.json"), p).map_err(|e| format!("写入点表失败：{e}"))?;
        Ok(())
    }

    // —— UI 插件（ESM 资源由 gatewayd 静态服务，注册表目录为真相源）——

    pub async fn ui_plugins(&self) -> Vec<UiPluginDto> {
        self.ui_plugins
            .read()
            .await
            .list()
            .iter()
            .map(ui_plugin_dto)
            .collect()
    }

    /// 已安装插件的真实目录（目录名不一定等于插件 id：手动拷贝也可能被扫描到）。
    pub async fn ui_plugin_dir(&self, id: &str) -> Option<PathBuf> {
        self.ui_plugins.read().await.get(id).map(|p| p.dir.clone())
    }

    /// 设备包 ESM 组件资源：只暴露清单声明 entry 所在目录下的文件。
    pub async fn package_widget_asset(
        &self,
        id: &str,
        version: &str,
        relative: &str,
    ) -> Option<PathBuf> {
        let registry = self.packages.read().await.clone();
        let pkg = registry.get(id)?;
        if pkg.manifest.version.to_string() != version {
            return None;
        }
        let rel = std::path::Path::new(relative);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return None;
        }
        let declared = pkg.manifest.ui.widgets.iter().any(|w| {
            let entry = std::path::Path::new(&w.entry);
            let dir = entry.parent().unwrap_or_else(|| std::path::Path::new(""));
            rel == entry || (!dir.as_os_str().is_empty() && rel.starts_with(dir))
        });
        if !declared {
            return None;
        }
        let base = std::fs::canonicalize(&pkg.dir).ok()?;
        let file = std::fs::canonicalize(pkg.dir.join(rel)).ok()?;
        if !file.starts_with(&base) || !file.is_file() {
            return None;
        }
        Some(file)
    }

    /// 安装/升级插件并返回最新列表。
    pub async fn import_ui_plugin(&self, path: &str) -> Result<Vec<UiPluginDto>, String> {
        install_ui_plugin(
            std::path::Path::new(path),
            &self.data_dir.join("ui-plugins"),
        )
        .map_err(|e| e.to_string())?;
        self.reload_ui_plugins().await;
        Ok(self.ui_plugins().await)
    }

    /// 删除插件目录并返回最新列表；不存在视为成功（幂等）。
    pub async fn remove_ui_plugin(&self, id: &str) -> Result<Vec<UiPluginDto>, String> {
        if !valid_id(id) {
            return Err(format!("插件 id 非法：{id}"));
        }
        let dir = {
            let registry = self.ui_plugins.read().await;
            registry
                .get(id)
                .map(|p| p.dir.clone())
                .unwrap_or_else(|| self.data_dir.join("ui-plugins").join(id))
        };
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("删除插件目录失败：{e}"))?;
        }
        self.reload_ui_plugins().await;
        Ok(self.ui_plugins().await)
    }

    async fn reload_ui_plugins(&self) {
        let registry = UiPluginRegistry::scan(&self.data_dir.join("ui-plugins"));
        for (path, why) in &registry.broken {
            tracing::warn!(plugin = %path.display(), reason = %why, "跳过损坏的 UI 插件");
        }
        *self.ui_plugins.write().await = registry;
    }

    // —— 仪表盘（布局属工程数据，落 <data-dir>/dashboards.json）——

    pub async fn dashboards(&self) -> Vec<DashboardDto> {
        self.dashboards.read().await.list()
    }

    pub async fn save_dashboard(
        &self,
        dashboard: DashboardDto,
    ) -> Result<Vec<DashboardDto>, String> {
        let mut store = self.dashboards.write().await;
        store.upsert(dashboard)?;
        Ok(store.list())
    }

    pub async fn remove_dashboard(&self, id: &str) -> Result<Vec<DashboardDto>, String> {
        let mut store = self.dashboards.write().await;
        store.remove(id)?;
        Ok(store.list())
    }

    // —— 设备类型工作区（用户覆盖包默认页面，落 <data-dir>/workspaces.json）——

    pub async fn workspaces(&self) -> Vec<WorkspaceDto> {
        self.workspaces.read().await.list()
    }

    pub async fn save_workspace(
        &self,
        workspace: WorkspaceDto,
    ) -> Result<Vec<WorkspaceDto>, String> {
        let mut store = self.workspaces.write().await;
        store.upsert(workspace)?;
        Ok(store.list())
    }

    pub async fn remove_workspace(&self, device_type: &str) -> Result<Vec<WorkspaceDto>, String> {
        let mut store = self.workspaces.write().await;
        store.remove(device_type)?;
        Ok(store.list())
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

/// 包/插件 id 白名单：字母数字 + `. - _`，且至少含一个字母数字
/// （挡住空串与 `.` / `..` 这类路径穿越）。
fn valid_id(id: &str) -> bool {
    id.chars().any(|c| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

/// 已安装包 → UI DTO（快照与包列表共用）。
fn package_dto(p: &InstalledPackage) -> PackageDto {
    PackageDto {
        id: p.manifest.id.clone(),
        name: p.manifest.name.clone(),
        vendor: p.manifest.vendor.clone(),
        version: p.manifest.version.to_string(),
        protocol: p.manifest.protocol.clone(),
        description: p.manifest.description.clone(),
        ui: PackageUiDto {
            pages: p
                .manifest
                .ui
                .pages
                .iter()
                .map(|page| DevicePageDto {
                    id: page.id.clone(),
                    title: page.title.clone(),
                    icon: page.icon.clone(),
                    components: page
                        .components
                        .iter()
                        .map(|comp| DevicePageComponentDto {
                            id: comp.id.clone(),
                            kind: comp.kind.clone(),
                            title: comp.title.clone(),
                            bind: comp
                                .bind
                                .iter()
                                .map(|(slot, spec)| (slot.clone(), binding_spec_dto(spec)))
                                .collect(),
                            rect: comp.rect.map(|r| hmi_wire::WidgetRectDto {
                                x: r.x,
                                y: r.y,
                                w: r.w,
                                h: r.h,
                            }),
                            config: comp.config.clone(),
                        })
                        .collect(),
                })
                .collect(),
            widgets: p
                .manifest
                .ui
                .widgets
                .iter()
                .map(|w| PackageWidgetDto {
                    id: w.id.clone(),
                    title: w.title.clone(),
                    description: w.description.clone(),
                    entry: w.entry.clone(),
                    size: WidgetSizeDto {
                        w: w.size.w,
                        h: w.size.h,
                        min_w: w.size.min_w,
                        min_h: w.size.min_h,
                    },
                    bindings: w
                        .bindings
                        .iter()
                        .map(|(slot, b)| {
                            (
                                slot.clone(),
                                hmi_wire::WidgetBindingDto {
                                    kind: b.kind.clone(),
                                    many: b.many,
                                    write: b.write,
                                    required: b.required,
                                },
                            )
                        })
                        .collect(),
                    config: w.config.clone(),
                    permissions: w.permissions.clone(),
                })
                .collect(),
        },
        points: p
            .points
            .points
            .iter()
            .map(|pt| PointDto {
                id: pt.id.clone(),
                name: pt.name.clone(),
                address: pt.address.clone(),
                area: pt.address.as_register().map(|r| r.area),
                data_type: pt.data_type,
                byte_order: pt.byte_order,
                access: pt.access,
                scale: pt.scale,
                offset: pt.offset,
                unit: pt.unit.clone(),
                poll_group: pt.poll_group.clone(),
                bits: pt
                    .bits
                    .iter()
                    .map(|(bit, b)| hmi_wire::BitDto {
                        bit: *bit,
                        name: b.name.clone(),
                        on: b.on.clone(),
                        off: b.off.clone(),
                    })
                    .collect(),
                min: pt.min,
                max: pt.max,
                step: pt.step,
                value_enum: pt
                    .value_enum
                    .iter()
                    .map(|e| hmi_wire::EnumValueDto {
                        value: e.value,
                        label: e.label.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// 槽位绑定规格 → DTO。
fn binding_spec_dto(spec: &PkgBindingSpec) -> BindingSpecDto {
    match spec {
        PkgBindingSpec::One(b) => BindingSpecDto::One(binding_dto(b)),
        PkgBindingSpec::Many(v) => BindingSpecDto::Many(v.iter().map(binding_dto).collect()),
    }
}

fn binding_dto(b: &hmi_packaging::BindingDecl) -> BindingDeclDto {
    BindingDeclDto {
        point: b.point.clone(),
        bit: b.bit,
        tag: b.tag.clone(),
    }
}

/// 解析包草稿并归一化（v2 widgets[] → components；包内组件短名 → 全局 kind）。
fn parse_package_draft(
    manifest_json: &serde_json::Value,
    points_json: &serde_json::Value,
) -> Result<(PackageManifest, PointsTable), String> {
    let mut manifest: PackageManifest = serde_json::from_value(manifest_json.clone())
        .map_err(|e| format!("manifest 解析失败：{e}"))?;
    manifest.ui.normalize(&manifest.id);
    let points: PointsTable =
        serde_json::from_value(points_json.clone()).map_err(|e| format!("点表解析失败：{e}"))?;
    Ok((manifest, points))
}

/// 已安装 UI 插件 → UI DTO。
fn ui_plugin_dto(p: &InstalledUiPlugin) -> UiPluginDto {
    let m = &p.manifest;
    UiPluginDto {
        id: m.id.clone(),
        name: m.name.clone(),
        version: m.version.to_string(),
        description: m.description.clone(),
        author: m.author.clone(),
        sdk_version: m.sdk_version,
        permissions: m.permissions.clone(),
        entry: m.entry.clone(),
        widgets: m
            .widgets
            .iter()
            .map(|w| UiWidgetDto {
                id: w.id.clone(),
                name: w.name.clone(),
                description: w.description.clone(),
                default_w: w.default_w,
                default_h: w.default_h,
            })
            .collect(),
        pages: m
            .pages
            .iter()
            .map(|p| UiPageDto {
                id: p.id.clone(),
                title: p.title.clone(),
                icon: p.icon.clone(),
            })
            .collect(),
        device_panels: m
            .device_panels
            .iter()
            .map(|p| UiDevicePanelDto {
                id: p.id.clone(),
                title: p.title.clone(),
            })
            .collect(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_id_rejects_path_traversal() {
        assert!(valid_id("com.acme.pm3000"));
        assert!(valid_id("a"));
        assert!(!valid_id(""));
        assert!(!valid_id("."));
        assert!(!valid_id(".."));
        assert!(!valid_id("..."));
        assert!(!valid_id("a/b"));
        assert!(!valid_id("a\\b"));
    }

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
}
