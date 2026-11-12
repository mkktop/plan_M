//! GatewayService：把配置存储、包注册表、驱动/传输注册表、通道监督器、TagHub
//! 编成一个可被 CLI/WS 层驱动的整体。
//!
//! 本模块只产出**领域视图**（[`Snapshot`] 等），不认识任何 wire DTO；
//! gatewayd 的 `dto` 模块负责映射成 WS 契约。子模块按职责拆分：
//! - [`channels`]：通道/设备配置变更、通道生命周期、写点、扫描
//! - [`packages`]：设备包安装/删除/草稿
//! - [`plugins`]：UI 插件注册表与包内 ESM 组件资源

mod channels;
mod packages;
mod plugins;

use crate::hub::TagHub;
use crate::store::{ConfigStore, JsonConfigStore};
use crate::supervisor::{ChannelHandle, GatewayEvent};
use hmi_domain::config::{ChannelConfig, DeviceConfig, ProjectConfig};
use hmi_domain::driver::Driver;
use hmi_domain::link::{TransportFactory, TransportKind, TransportTarget};
use hmi_domain::value::{Quality, TagChange, TagId};
use hmi_packaging::{InstalledPackage, PackageRegistry, UiPluginRegistry};
use semver::Version;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// 通道的运行时状态视图（wire 映射在 gatewayd）。
pub struct ChannelStatus {
    pub config: ChannelConfig,
    /// running / recovering / faulted / stopped
    pub state: String,
    pub config_error: Option<String>,
}

/// 设备的运行时状态视图。
pub struct DeviceStatus {
    pub config: DeviceConfig,
    pub online: bool,
    pub config_error: Option<String>,
}

/// 全量快照（领域视图）。
pub struct Snapshot {
    pub channels: Vec<ChannelStatus>,
    pub devices: Vec<DeviceStatus>,
    pub packages: Vec<InstalledPackage>,
    pub tags: Vec<TagChange>,
}

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

    /// 宿主注册的驱动（Welcome 能力协商用）。
    pub fn drivers(&self) -> &[Arc<dyn Driver>] {
        &self.driver_list
    }

    /// 宿主支持的传输种类（Welcome 能力协商）。
    pub fn transport_kinds(&self) -> Vec<TransportKind> {
        let mut kinds: Vec<TransportKind> = self.transports.keys().copied().collect();
        kinds.sort_by_key(|k| k.as_str());
        kinds
    }

    /// 某种传输的可发现目标（串口列表等）。
    pub async fn transport_targets(&self, kind: TransportKind) -> Vec<TransportTarget> {
        match self.transports.get(&kind) {
            Some(f) => f.targets().await,
            None => Vec::new(),
        }
    }

    /// 全量快照（领域视图；wire 映射见 gatewayd `dto`）。
    pub async fn snapshot(&self) -> Snapshot {
        let cfg = self.config.read().await;
        let channels = self.channels.read().await;
        let packages = self.packages.read().await.clone();
        let channel_errors = self.channel_errors.read().await;
        let device_errors = self.device_errors.read().await;

        Snapshot {
            channels: cfg
                .channels
                .iter()
                .map(|c| ChannelStatus {
                    config: c.clone(),
                    state: channels
                        .get(&c.id)
                        .map(|h| h.state())
                        .unwrap_or_else(|| "stopped".into()),
                    config_error: channel_errors.get(&c.id).cloned(),
                })
                .collect(),
            devices: cfg
                .devices
                .iter()
                .map(|d| DeviceStatus {
                    config: d.clone(),
                    online: device_online_of(&d.id, &d.channel_id, &channels),
                    config_error: device_errors.get(&d.id).cloned(),
                })
                .collect(),
            packages: packages.list().to_vec(),
            tags: self.hub.snapshot_all(),
        }
    }

    /// 订阅事件流（通道状态 / 设备上下线）。
    pub fn subscribe_events(&self) -> broadcast::Receiver<GatewayEvent> {
        self.events.subscribe()
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

/// 包/插件 id 白名单：字母数字 + `. - _`，且至少含一个字母数字
/// （挡住空串与 `.` / `..` 这类路径穿越）。
pub(crate) fn valid_id(id: &str) -> bool {
    id.chars().any(|c| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
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
}
