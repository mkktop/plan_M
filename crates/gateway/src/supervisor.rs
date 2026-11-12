//! 通道监督器：一条 Link（串口/TCP/SIM/BLE）的全生命周期。
//!
//! 职责：
//! - 异步打开/断线重连（指数退避，封顶 5s；期间通道内全部 tag → CommLost）
//! - 事务严格串行（半双工/单连接硬约束）
//! - 轮询调度：按驱动编译出的组周期执行 `Driver::poll`
//! - 错误分类：链路死亡 → 重连；设备级错误 → 重试 → 离线 → 周期探测
//! - 外部命令（写点/探测）插入事务队列执行
//!
//! 本模块只认识 `TransportFactory` 与 `Driver` 端口，不知道 Modbus/串口细节。

use crate::hub::TagHub;
use hmi_domain::config::{ChannelConfig, DeviceAddress};
use hmi_domain::driver::{DevicePlan, Driver, PollClass, PollGroup, Sample};
use hmi_domain::link::{Link, TransportFactory};
use hmi_domain::points::PointDef;
use hmi_domain::protocol::ProtocolDecl;
use hmi_domain::value::{now_ms, Quality, TagId, TagSnapshot, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

/// 注入给监督器的命令。
pub enum SupervisorCmd {
    Write {
        driver: Arc<dyn Driver>,
        address: DeviceAddress,
        point: Box<PointDef>,
        value: Value,
        protocol: ProtocolDecl,
        resp: oneshot::Sender<Result<(), String>>,
    },
    Probe {
        driver: Arc<dyn Driver>,
        address: DeviceAddress,
        protocol: ProtocolDecl,
        resp: oneshot::Sender<bool>,
    },
    /// 停止通道（释放连接）
    Stop,
}

/// 通道级共享状态（服务层读快照用）。
#[derive(Debug, Default)]
pub struct ChannelShared {
    /// running / recovering / faulted / stopped
    pub state: std::sync::RwLock<String>,
    /// 各设备在线标志（device_id → online）
    pub devices_online: std::sync::RwLock<std::collections::HashMap<String, Arc<AtomicBool>>>,
}

impl ChannelShared {
    pub fn new(device_ids: &[String]) -> Arc<Self> {
        let map = device_ids
            .iter()
            .map(|id| (id.clone(), Arc::new(AtomicBool::new(true))))
            .collect();
        Arc::new(Self {
            state: std::sync::RwLock::new("starting".into()),
            devices_online: std::sync::RwLock::new(map),
        })
    }

    pub fn state(&self) -> String {
        self.state.read().unwrap().clone()
    }

    fn set_state(&self, s: &str) {
        *self.state.write().unwrap() = s.to_string();
    }

    fn set_device_online(&self, device: &str, online: bool) {
        if let Some(f) = self.devices_online.read().unwrap().get(device) {
            f.store(online, Ordering::Relaxed);
        }
    }
}

/// 事件（服务层 broadcast 给 WS 层）。
#[derive(Clone, Debug, PartialEq)]
pub enum GatewayEvent {
    ChannelState {
        channel: String,
        state: String,
        detail: String,
    },
    DeviceOnline {
        device: String,
        online: bool,
    },
}

/// 一个设备的运行时物料。
pub struct DeviceRuntime {
    pub device_id: String,
    pub address: DeviceAddress,
    pub driver: Arc<dyn Driver>,
    /// 设备包的协议声明（写点/探测时解析驱动私有参数）
    pub protocol: ProtocolDecl,
    pub plan: DevicePlan,
    pub all_tags: Vec<TagId>,
}

/// 通道运行时物料。
pub struct ChannelRuntime {
    pub config: ChannelConfig,
    pub devices: Vec<DeviceRuntime>,
    pub factory: Arc<dyn TransportFactory>,
    pub hub: Arc<TagHub>,
    pub events: broadcast::Sender<GatewayEvent>,
}

#[derive(Error, Debug)]
enum PollFailure {
    #[error("link dead: {0}")]
    LinkDead(String),
    #[error("device down: {0}")]
    DeviceDown(String),
    #[error("device fatal: {0}")]
    DeviceFatal(String),
}

#[derive(Clone, Copy)]
struct Slot {
    device: usize,
    group: usize,
    next: Instant,
}

/// 通道句柄。
#[derive(Clone)]
pub struct ChannelHandle {
    pub tx: mpsc::Sender<SupervisorCmd>,
    pub shared: Arc<ChannelShared>,
    join: Arc<std::sync::Mutex<Option<JoinHandle<()>>>>,
}

impl ChannelHandle {
    pub fn state(&self) -> String {
        self.shared.state()
    }

    /// 停止通道并等待监督器任务退出（释放串口/连接，避免重开竞态）。
    pub async fn stop(&self) {
        let _ = self.tx.send(SupervisorCmd::Stop).await;
        let handle = self.join.lock().unwrap().take();
        if let Some(h) = handle {
            let _ = tokio::time::timeout(Duration::from_secs(2), h).await;
        }
    }

    pub async fn write(
        &self,
        driver: Arc<dyn Driver>,
        address: DeviceAddress,
        point: PointDef,
        value: Value,
        protocol: ProtocolDecl,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(SupervisorCmd::Write {
                driver,
                address,
                point: Box::new(point),
                value,
                protocol,
                resp: tx,
            })
            .await
            .map_err(|_| "通道已停止".to_string())?;
        rx.await.map_err(|_| "通道无应答".to_string())?
    }

    pub async fn probe(
        &self,
        driver: Arc<dyn Driver>,
        address: DeviceAddress,
        protocol: ProtocolDecl,
    ) -> bool {
        let (tx, rx) = oneshot::channel();
        if self
            .tx
            .send(SupervisorCmd::Probe {
                driver,
                address,
                protocol,
                resp: tx,
            })
            .await
            .is_err()
        {
            return false;
        }
        rx.await.unwrap_or(false)
    }
}

/// 启动一个通道监督器任务。
pub fn spawn_channel(rt: ChannelRuntime, cmd_capacity: usize) -> ChannelHandle {
    let shared = ChannelShared::new(
        &rt.devices
            .iter()
            .map(|d| d.device_id.clone())
            .collect::<Vec<_>>(),
    );
    let (tx, rx) = mpsc::channel(cmd_capacity);
    let join = Arc::new(std::sync::Mutex::new(None));
    let handle = ChannelHandle {
        tx,
        shared: shared.clone(),
        join: join.clone(),
    };
    let task_shared = shared.clone();
    let h = tokio::spawn(async move {
        run_channel(rt, rx, task_shared).await;
    });
    *join.lock().unwrap() = Some(h);
    handle
}

async fn run_channel(
    rt: ChannelRuntime,
    mut rx: mpsc::Receiver<SupervisorCmd>,
    shared: Arc<ChannelShared>,
) {
    let timeout = Duration::from_millis(rt.config.timing.resp_timeout_ms as u64);
    let gap = rt.config.inter_frame_gap();
    let retries = rt.config.timing.retries;
    let probe_timeout = timeout.mul_f64(0.75);
    let probe_interval = Duration::from_millis(rt.config.timing.probe_interval_ms);
    let channel_id = rt.config.id.clone();
    let mut backoff = Duration::from_secs(1);

    loop {
        shared.set_state("recovering");
        let open_result = open_with_cmds(&rt.factory, &rt.config, &mut rx).await;
        let link = match open_result {
            OpenOutcome::Opened(link) => link,
            OpenOutcome::Stop => {
                shared.set_state("stopped");
                return;
            }
            OpenOutcome::Failed(e) => {
                tracing::warn!(channel = %channel_id, error = %e, "打开通道失败，退避重试");
                mark_channel_lost(&rt, &shared);
                if sleep_with_cmds(&mut rx, backoff, &shared).await {
                    return;
                }
                backoff = (backoff * 2).min(Duration::from_secs(5));
                continue;
            }
        };
        backoff = Duration::from_secs(1);
        shared.set_state("running");

        // 每个设备 × 每个轮询组一个槽位
        let mut slots: Vec<Slot> = Vec::new();
        for (di, dev) in rt.devices.iter().enumerate() {
            for (gi, _) in dev.plan.groups.iter().enumerate() {
                slots.push(Slot {
                    device: di,
                    group: gi,
                    next: Instant::now(),
                });
            }
        }
        let mut offline = vec![false; rt.devices.len()];
        let mut next_probe = vec![Instant::now(); rt.devices.len()];

        let inner = loop {
            let now = Instant::now();
            let next_due = slots
                .iter()
                .map(|s| s.next)
                .min()
                .unwrap_or(now + Duration::from_secs(3600));

            tokio::select! {
                cmd = rx.recv() => {
                    match cmd {
                        None | Some(SupervisorCmd::Stop) => {
                            shared.set_state("stopped");
                            return;
                        }
                        Some(SupervisorCmd::Write { driver, address, point, value, protocol, resp }) => {
                            match driver
                                .write(&*link, &address, &point, &value, &protocol, timeout)
                                .await
                            {
                                Ok(()) => { let _ = resp.send(Ok(())); sleep_gap(gap).await; }
                                Err(e) => {
                                    let dead = e.poll_class() == PollClass::LinkDead;
                                    let _ = resp.send(Err(e.to_string()));
                                    if dead { break PollFailure::LinkDead(e.to_string()); }
                                    sleep_gap(gap).await;
                                }
                            }
                        }
                        Some(SupervisorCmd::Probe { driver, address, protocol, resp }) => {
                            let ok = driver
                                .probe(&*link, &address, &protocol, probe_timeout)
                                .await
                                .is_ok();
                            let _ = resp.send(ok);
                            sleep_gap(gap).await;
                        }
                    }
                }
                _ = tokio::time::sleep_until(next_due.into()) => {
                    // 执行所有到期槽位（单任务串行）
                    let now = Instant::now();
                    let mut failure: Option<PollFailure> = None;
                    for idx in 0..slots.len() {
                        if slots[idx].next > now {
                            continue;
                        }
                        let dev = &rt.devices[slots[idx].device];
                        let group = &dev.plan.groups[slots[idx].group];

                        if offline[slots[idx].device] {
                            if now >= next_probe[slots[idx].device] {
                                match dev.driver.probe(&*link, &dev.address, &dev.protocol, probe_timeout).await {
                                    Ok(()) => {
                                        offline[slots[idx].device] = false;
                                        shared.set_device_online(&dev.device_id, true);
                                        let _ = rt.events.send(GatewayEvent::DeviceOnline {
                                            device: dev.device_id.clone(),
                                            online: true,
                                        });
                                        slots[idx].next = now;
                                    }
                                    Err(e) if e.poll_class() == PollClass::LinkDead => {
                                        failure = Some(PollFailure::LinkDead(e.to_string()));
                                        break;
                                    }
                                    Err(_) => {
                                        next_probe[slots[idx].device] = now + probe_interval;
                                        slots[idx].next = now + probe_interval;
                                    }
                                }
                            } else {
                                slots[idx].next = next_probe[slots[idx].device];
                            }
                            continue;
                        }

                        match exec_group(&*link, dev, group, timeout, retries, gap).await {
                            Ok(samples) => {
                                write_samples(&rt.hub, samples);
                                slots[idx].next = Instant::now() + group.interval;
                            }
                            Err(PollFailure::LinkDead(e)) => {
                                failure = Some(PollFailure::LinkDead(e));
                                break;
                            }
                            Err(PollFailure::DeviceFatal(e)) => {
                                tracing::warn!(device = %dev.device_id, error = %e, "设备不可恢复，标记离线");
                                mark_device_offline(&rt, &shared, slots[idx].device);
                                offline[slots[idx].device] = true;
                                next_probe[slots[idx].device] = Instant::now() + probe_interval;
                                slots[idx].next = next_probe[slots[idx].device];
                            }
                            Err(PollFailure::DeviceDown(e)) => {
                                tracing::warn!(device = %dev.device_id, error = %e, "设备重试耗尽，标记离线");
                                mark_device_offline(&rt, &shared, slots[idx].device);
                                offline[slots[idx].device] = true;
                                next_probe[slots[idx].device] = Instant::now() + probe_interval;
                                slots[idx].next = next_probe[slots[idx].device];
                            }
                        }
                    }
                    if let Some(f) = failure {
                        break f;
                    }
                }
            }
        };

        shared.set_state("recovering");
        mark_channel_lost(&rt, &shared);
        tracing::warn!(channel = %channel_id, error = %inner, "通道中止，准备重连");
        drop(link);
        if sleep_with_cmds(&mut rx, backoff, &shared).await {
            return;
        }
    }
}

enum OpenOutcome {
    Opened(Box<dyn Link>),
    Failed(String),
    Stop,
}

async fn open_with_cmds(
    factory: &Arc<dyn TransportFactory>,
    config: &ChannelConfig,
    rx: &mut mpsc::Receiver<SupervisorCmd>,
) -> OpenOutcome {
    let fut = factory.open(config);
    tokio::pin!(fut);
    loop {
        tokio::select! {
            r = &mut fut => {
                return match r {
                    Ok(link) => OpenOutcome::Opened(link),
                    Err(e) => OpenOutcome::Failed(e.to_string()),
                };
            }
            cmd = rx.recv() => {
                match cmd {
                    None | Some(SupervisorCmd::Stop) => return OpenOutcome::Stop,
                    Some(SupervisorCmd::Write { resp, .. }) => {
                        let _ = resp.send(Err("通道正在重连".into()));
                    }
                    Some(SupervisorCmd::Probe { resp, .. }) => {
                        let _ = resp.send(false);
                    }
                }
            }
        }
    }
}

async fn exec_group(
    link: &dyn Link,
    dev: &DeviceRuntime,
    group: &PollGroup,
    timeout: Duration,
    retries: u32,
    gap: Duration,
) -> Result<Vec<Sample>, PollFailure> {
    let mut last = String::new();
    for attempt in 0..=retries {
        match dev.driver.poll(link, &dev.address, group, timeout).await {
            Ok(samples) => {
                sleep_gap(gap).await;
                return Ok(samples);
            }
            Err(e) => match e.poll_class() {
                PollClass::LinkDead => return Err(PollFailure::LinkDead(e.to_string())),
                PollClass::Fatal => return Err(PollFailure::DeviceFatal(e.to_string())),
                PollClass::Retry => {
                    last = e.to_string();
                    tracing::debug!(device = %dev.device_id, attempt, error = %e, "事务失败");
                    if attempt < retries {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                }
            },
        }
    }
    Err(PollFailure::DeviceDown(last))
}

fn write_samples(hub: &TagHub, samples: Vec<Sample>) {
    let ts = now_ms();
    let batch: Vec<(TagId, TagSnapshot)> = samples
        .into_iter()
        .map(|s| {
            (
                s.tag,
                TagSnapshot {
                    value: s.value,
                    quality: s.quality,
                    ts_ms: ts,
                },
            )
        })
        .collect();
    hub.write(batch);
}

fn mark_device_offline(rt: &ChannelRuntime, shared: &Arc<ChannelShared>, dev_idx: usize) {
    let dev = &rt.devices[dev_idx];
    shared.set_device_online(&dev.device_id, false);
    let _ = rt.events.send(GatewayEvent::DeviceOnline {
        device: dev.device_id.clone(),
        online: false,
    });
    hub_lost(&rt.hub, &dev.all_tags, now_ms());
}

fn mark_channel_lost(rt: &ChannelRuntime, shared: &Arc<ChannelShared>) {
    let ts = now_ms();
    for dev in &rt.devices {
        shared.set_device_online(&dev.device_id, false);
        let _ = rt.events.send(GatewayEvent::DeviceOnline {
            device: dev.device_id.clone(),
            online: false,
        });
        hub_lost(&rt.hub, &dev.all_tags, ts);
    }
}

fn hub_lost(hub: &TagHub, tags: &[TagId], ts: u64) {
    let batch: Vec<(TagId, TagSnapshot)> = tags
        .iter()
        .map(|tag| {
            let last = hub.get(tag);
            let value = last.map(|s| s.value).unwrap_or(Value::F64(0.0));
            (
                tag.clone(),
                TagSnapshot {
                    value,
                    quality: Quality::CommLost,
                    ts_ms: ts,
                },
            )
        })
        .collect();
    hub.write(batch);
}

async fn sleep_gap(gap: Duration) {
    if !gap.is_zero() {
        tokio::time::sleep(gap).await;
    }
}

/// 重连等待期间仍消费命令（写/探测直接失败），Stop 立刻退出。
/// 返回 true 表示收到 Stop。
async fn sleep_with_cmds(
    rx: &mut mpsc::Receiver<SupervisorCmd>,
    duration: Duration,
    shared: &Arc<ChannelShared>,
) -> bool {
    let deadline = Instant::now() + duration;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        tokio::select! {
            _ = tokio::time::sleep(deadline - now) => return false,
            cmd = rx.recv() => {
                match cmd {
                    None | Some(SupervisorCmd::Stop) => {
                        shared.set_state("stopped");
                        return true;
                    }
                    Some(SupervisorCmd::Write { resp, .. }) => {
                        let _ = resp.send(Err("总线未连接".into()));
                    }
                    Some(SupervisorCmd::Probe { resp, .. }) => {
                        let _ = resp.send(false);
                    }
                }
            }
        }
    }
}
