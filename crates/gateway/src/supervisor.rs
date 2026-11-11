//! 通道监督器：一条 485 总线（一个串口）的全生命周期。
//!
//! 职责：
//! - 总线打开 / 断线重连（指数退避，封顶 5s；期间通道内全部 tag → CommLost）
//! - 事务严格串行（半双工总线的硬约束——单任务独占总线天然保证）
//! - 轮询调度：按组周期执行"读块"事务，解码写 TagHub
//! - 重试与从站离线：单事务超时/协议错重试 N 次，超限标记离线、定期探测恢复；
//!   一台坏设备不阻塞同总线其他设备
//! - 外部命令（写点/探测）插入事务队列执行
//!
//! 总线打开策略由外部注入（`BusOpener`），本 crate 不知道串口细节——
//! 测试注入虚拟总线，gatewayd 注入串口/SIM 工厂。

use crate::hub::TagHub;
use crate::poll::{DevicePollPlan, PollGroupPlan};
use hmi_domain::config::SerialChannelConfig;
use hmi_domain::driver::{Driver, DriverError};
use hmi_domain::transport::{BusError, BusTransport};
use hmi_domain::value::TagId;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

/// 打开总线的工厂：入参为通道配置（串口参数/SIM 标记都在里面），
/// 每次重连调用一次；返回 Err 表示本次打开失败（原因可读）。
pub type BusOpener =
    Arc<dyn Fn(&SerialChannelConfig) -> Result<Box<dyn BusTransport>, String> + Send + Sync>;

/// 注入给监督器的命令。
pub enum SupervisorCmd {
    WriteRegs {
        slave: u8,
        start: u16,
        words: Vec<u16>,
        driver: Arc<dyn Driver>,
        resp: oneshot::Sender<Result<(), String>>,
    },
    WriteCoil {
        slave: u8,
        bit: u16,
        value: bool,
        driver: Arc<dyn Driver>,
        resp: oneshot::Sender<Result<(), String>>,
    },
    Probe {
        slave: u8,
        driver: Arc<dyn Driver>,
        resp: oneshot::Sender<bool>,
    },
    /// 停止通道（释放串口）
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

/// 一个设备的运行时物料：从站地址 + 驱动 + 编译好的计划 + 全部 tag。
/// 驱动跟随设备包声明 —— 同一条总线上允许混协议设备。
pub struct DeviceRuntime {
    pub device_id: String,
    pub slave: u8,
    pub driver: Arc<dyn Driver>,
    pub plan: DevicePollPlan,
    pub all_tags: Vec<TagId>,
}

/// 通道运行时物料。
pub struct ChannelRuntime {
    pub cfg: SerialChannelConfig,
    pub devices: Vec<DeviceRuntime>,
    pub open_bus: BusOpener,
    pub hub: Arc<TagHub>,
    pub events: tokio::sync::broadcast::Sender<GatewayEvent>,
}

#[derive(Error, Debug)]
enum PollFailure {
    /// 从站级故障（超时/协议错，重试耗尽）→ 标记该从站离线
    #[error("slave down: {0}")]
    SlaveDown(String),
    /// 总线级故障（串口 I/O 错误）→ 重连
    #[error("bus dead: {0}")]
    BusDead(String),
}

struct Slot {
    device: usize,
    group: usize,
    next: tokio::time::Instant,
}

/// 通道句柄。
#[derive(Clone)]
pub struct ChannelHandle {
    pub tx: mpsc::Sender<SupervisorCmd>,
    pub shared: Arc<ChannelShared>,
}

impl ChannelHandle {
    pub fn state(&self) -> String {
        self.shared.state()
    }

    pub async fn write_regs(
        &self,
        driver: Arc<dyn Driver>,
        slave: u8,
        start: u16,
        words: Vec<u16>,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(SupervisorCmd::WriteRegs {
                slave,
                start,
                words,
                driver,
                resp: tx,
            })
            .await
            .map_err(|_| "通道已停止".to_string())?;
        rx.await.map_err(|_| "通道无应答".to_string())?
    }

    pub async fn write_coil(
        &self,
        driver: Arc<dyn Driver>,
        slave: u8,
        bit: u16,
        value: bool,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(SupervisorCmd::WriteCoil {
                slave,
                bit,
                value,
                driver,
                resp: tx,
            })
            .await
            .map_err(|_| "通道已停止".to_string())?;
        rx.await.map_err(|_| "通道无应答".to_string())?
    }

    pub async fn probe(&self, driver: Arc<dyn Driver>, slave: u8) -> Result<bool, String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(SupervisorCmd::Probe {
                slave,
                driver,
                resp: tx,
            })
            .await
            .map_err(|_| "通道已停止".to_string())?;
        rx.await.map_err(|_| "通道无应答".to_string())
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
    let handle = ChannelHandle {
        tx: tx.clone(),
        shared: shared.clone(),
    };
    let task_shared = shared.clone();
    tokio::spawn(async move {
        run_channel(rt, rx, task_shared).await;
    });
    handle
}

async fn run_channel(
    rt: ChannelRuntime,
    mut cmd_rx: mpsc::Receiver<SupervisorCmd>,
    shared: Arc<ChannelShared>,
) {
    let channel_id = rt.cfg.id.clone();
    let timeout = Duration::from_millis(rt.cfg.resp_timeout_ms as u64);
    let gap = Duration::from_millis(rt.cfg.inter_frame_gap_ms);
    let retries = rt.cfg.retries as u32;
    let probe_timeout = Duration::from_millis((rt.cfg.resp_timeout_ms as f64 * 0.75) as u64);
    let probe_interval = Duration::from_secs(5);

    tracing::info!(channel = %channel_id, port = %rt.cfg.port, devices = rt.devices.len(), "通道启动");

    let mut backoff = Duration::from_secs(1);
    let mut marked_lost = false;

    'outer: loop {
        // —— 打开总线（失败 → 退避重试，命令仍可响应）——
        let bus: Box<dyn BusTransport> = match (rt.open_bus)(&rt.cfg) {
            Ok(b) => b,
            Err(e) => {
                if !marked_lost {
                    mark_channel_lost(&rt, &shared);
                    marked_lost = true;
                }
                shared.set_state("recovering");
                emit_state(&rt, &channel_id, "recovering", &e);
                match sleep_with_cmds(&mut cmd_rx, backoff).await {
                    Flow::Continue => {}
                    Flow::Stop => break 'outer,
                }
                backoff = (backoff * 2).min(Duration::from_secs(5));
                continue;
            }
        };
        marked_lost = false;
        shared.set_state("running");
        emit_state(&rt, &channel_id, "running", "");

        // —— 轮询主循环 ——
        let mut slots: Vec<Slot> = Vec::new();
        for (di, d) in rt.devices.iter().enumerate() {
            for (gi, _) in d.plan.groups.iter().enumerate() {
                slots.push(Slot {
                    device: di,
                    group: gi,
                    next: tokio::time::Instant::now(),
                });
            }
        }
        let mut offline: Vec<bool> = vec![false; rt.devices.len()];
        let mut next_probe = vec![tokio::time::Instant::now(); rt.devices.len()];

        let bus_ref = bus.as_ref();
        loop {
            let now = tokio::time::Instant::now();
            let next_due = slots
                .iter()
                .map(|s| s.next)
                .min()
                .unwrap_or(now + Duration::from_secs(1));

            tokio::select! {
                biased;
                cmd = cmd_rx.recv() => {
                    match cmd {
                        None | Some(SupervisorCmd::Stop) => {
                            shared.set_state("stopped");
                            emit_state(&rt, &channel_id, "stopped", "");
                            break 'outer;
                        }
                        Some(SupervisorCmd::WriteRegs { slave, start, words, driver, resp }) => {
                            let r = driver
                                .write_registers(bus_ref, slave, start, &words, timeout)
                                .await
                                .map_err(|e| e.to_string());
                            let _ = resp.send(r);
                            tokio::time::sleep(gap).await;
                        }
                        Some(SupervisorCmd::WriteCoil { slave, bit, value, driver, resp }) => {
                            let r = driver
                                .write_coil(bus_ref, slave, bit, value, timeout)
                                .await
                                .map_err(|e| e.to_string());
                            let _ = resp.send(r);
                            tokio::time::sleep(gap).await;
                        }
                        Some(SupervisorCmd::Probe { slave, driver, resp }) => {
                            let ok = driver.probe(bus_ref, slave, probe_timeout).await.is_ok();
                            let _ = resp.send(ok);
                            tokio::time::sleep(gap).await;
                        }
                    }
                }
                _ = tokio::time::sleep_until(next_due) => {
                    let mut now = tokio::time::Instant::now();
                    // 一口气消化所有到期槽位
                    loop {
                        let due: Vec<usize> = (0..slots.len())
                            .filter(|&i| slots[i].next <= now)
                            .collect();
                        if due.is_empty() {
                            break;
                        }
                        for i in due {
                            let di = slots[i].device;
                            let gi = slots[i].group;
                            let dev = &rt.devices[di];
                            let group = &dev.plan.groups[gi];

                            if offline[di] {
                                if now >= next_probe[di] {
                                    match dev.driver.probe(bus_ref, dev.slave, probe_timeout).await {
                                        Ok(()) => {
                                            offline[di] = false;
                                            shared.set_device_online(&dev.device_id, true);
                                            emit_online(&rt, &dev.device_id, true);
                                            slots[i].next = now; // 立即恢复轮询
                                            for s in slots.iter_mut().filter(|s| s.device == di) {
                                                if s.next > now { s.next = now; }
                                            }
                                        }
                                        Err(_) => {
                                            next_probe[di] = now + probe_interval;
                                            slots[i].next = next_probe[di];
                                        }
                                    }
                                } else {
                                    slots[i].next = next_probe[di];
                                }
                                continue;
                            }

                            match exec_group(bus_ref, dev.driver.as_ref(), dev.slave, group, timeout, retries, gap).await {
                                Ok(batch) => {
                                    rt.hub.write(batch);
                                    slots[i].next = now + group.interval;
                                }
                                Err(PollFailure::SlaveDown(why)) => {
                                    tracing::warn!(
                                        channel = %channel_id, device = %dev.device_id,
                                        reason = %why, "从站离线（重试耗尽）"
                                    );
                                    offline[di] = true;
                                    shared.set_device_online(&dev.device_id, false);
                                    emit_online(&rt, &dev.device_id, false);
                                    rt.hub.write(rt.hub.comm_lost_batch(&dev.all_tags));
                                    next_probe[di] = now + probe_interval;
                                    for s in slots.iter_mut().filter(|s| s.device == di) {
                                        s.next = next_probe[di];
                                    }
                                }
                                Err(PollFailure::BusDead(e)) => {
                                    tracing::error!(channel = %channel_id, error = %e, "总线故障，进入重连");
                                    shared.set_state("recovering");
                                    emit_state(&rt, &channel_id, "recovering", &e);
                                    mark_channel_lost(&rt, &shared);
                                    marked_lost = true;
                                    break;
                                }
                            }
                        }
                        now = tokio::time::Instant::now();
                    }
                }
            }

            // BusDead 冒泡：跳出轮询循环进入重连
            if shared.state() == "recovering" && marked_lost {
                break;
            }
        }
    }

    tracing::info!(channel = %channel_id, "通道退出");
}

/// 执行一个轮询组事务：读块 + 重试。
async fn exec_group(
    bus: &dyn BusTransport,
    driver: &dyn Driver,
    slave: u8,
    group: &PollGroupPlan,
    timeout: Duration,
    retries: u32,
    gap: Duration,
) -> Result<Vec<(hmi_domain::value::TagId, hmi_domain::value::TagSnapshot)>, PollFailure> {
    let mut last_err: Option<String> = None;
    for attempt in 0..=retries {
        match driver.read_blocks(bus, slave, &group.blocks, timeout).await {
            Ok(data) => {
                let batch = group.decode("", &data);
                if attempt > 0 {
                    tracing::debug!(slave, attempt, "事务重试后成功");
                }
                tokio::time::sleep(gap).await;
                return Ok(batch);
            }
            Err(DriverError::Bus(BusError::Io(e))) => {
                return Err(PollFailure::BusDead(e));
            }
            Err(e) => {
                last_err = Some(e.to_string());
                if attempt < retries {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
        }
    }
    Err(PollFailure::SlaveDown(
        last_err.unwrap_or_else(|| "未知".into()),
    ))
}

enum Flow {
    Continue,
    Stop,
}

/// 重连等待期间响应命令（无总线 → 写/探测返回失败）。
async fn sleep_with_cmds(cmd_rx: &mut mpsc::Receiver<SupervisorCmd>, dur: Duration) -> Flow {
    tokio::select! {
        _ = tokio::time::sleep(dur) => Flow::Continue,
        cmd = cmd_rx.recv() => {
            match cmd {
                None | Some(SupervisorCmd::Stop) => Flow::Stop,
                Some(SupervisorCmd::WriteRegs { resp, .. }) => {
                    let _ = resp.send(Err("总线未连接".into()));
                    Flow::Continue
                }
                Some(SupervisorCmd::WriteCoil { resp, .. }) => {
                    let _ = resp.send(Err("总线未连接".into()));
                    Flow::Continue
                }
                Some(SupervisorCmd::Probe { resp, .. }) => {
                    let _ = resp.send(false);
                    Flow::Continue
                }
            }
        }
    }
}

fn mark_channel_lost(rt: &ChannelRuntime, shared: &Arc<ChannelShared>) {
    let mut batch = Vec::new();
    for d in &rt.devices {
        batch.extend(rt.hub.comm_lost_batch(&d.all_tags));
        shared.set_device_online(&d.device_id, false);
    }
    rt.hub.write(batch);
}

fn emit_state(rt: &ChannelRuntime, channel: &str, state: &str, detail: &str) {
    let _ = rt.events.send(GatewayEvent::ChannelState {
        channel: channel.to_string(),
        state: state.to_string(),
        detail: detail.to_string(),
    });
}

fn emit_online(rt: &ChannelRuntime, device: &str, online: bool) {
    let _ = rt.events.send(GatewayEvent::DeviceOnline {
        device: device.to_string(),
        online,
    });
}

/// 共享总线适配器：把 `Arc<dyn BusTransport>` 再包一层供多个消费者使用
/// （测试与 SIM 演示：工厂每次返回同一总线的克隆视图）。
pub struct SharedBus(pub Arc<dyn BusTransport>);

#[async_trait::async_trait]
impl BusTransport for SharedBus {
    async fn transact(&self, request: &[u8], timeout: Duration) -> Result<Vec<u8>, BusError> {
        self.0.transact(request, timeout).await
    }
}
