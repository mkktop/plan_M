//! 协议驱动端口。
//!
//! 驱动负责"协议语义"的全部内容：
//! - 校验设备包点表（地址空间、可写区域等）；
//! - 把点表编译成协议私有的轮询计划（读块合并、特性订阅等）；
//! - 在 [`Link`] 上执行轮询/写入/探测。
//!
//! 调度、重试、质量戳、TagHub、配置/设备生命周期全部在 gateway，
//! 驱动不知道通道的传输类型，只通过 `link.kind()` 选择帧封装。

use crate::config::DeviceAddress;
use crate::link::{Link, LinkError, TransportKind};
use crate::points::{PointDef, PointsTable};
use crate::protocol::{DriverIssues, ProtocolDecl};
use crate::value::{Quality, TagId, Value};
use async_trait::async_trait;
use serde_json::Value as Json;
use std::any::Any;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum DriverError {
    /// 传输层错误；`Closed/Io` 视为链路死亡，`Timeout` 视为设备级可重试。
    #[error("link: {0}")]
    Link(#[from] LinkError),
    /// 设备级可重试错误（应答超时、从站异常、帧损坏等）
    #[error("device: {0}")]
    Device(String),
    /// 设备级不可重试错误（非法地址等）→ 直接标记离线
    #[error("fatal: {0}")]
    Fatal(String),
    /// 编译/配置错误（驱动缺失、点表与驱动不匹配）→ 跳过设备
    #[error("compile: {0}")]
    Compile(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
}

/// 轮询失败后的处理方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollClass {
    /// 链路死亡 → 整通道重连
    LinkDead,
    /// 设备级错误 → 重试，重试耗尽标记离线
    Retry,
    /// 不可恢复 → 直接标记离线，不再重试
    Fatal,
}

impl DriverError {
    pub fn poll_class(&self) -> PollClass {
        match self {
            DriverError::Link(LinkError::Closed | LinkError::Io(_)) => PollClass::LinkDead,
            DriverError::Link(LinkError::Timeout) => PollClass::Retry,
            DriverError::Device(_) => PollClass::Retry,
            DriverError::Fatal(_) | DriverError::Compile(_) | DriverError::Unsupported(_) => {
                PollClass::Fatal
            }
        }
    }
}

/// 驱动产出的一个 tag 值。
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub tag: TagId,
    pub value: Value,
    pub quality: Quality,
}

impl Sample {
    pub fn good(tag: TagId, value: Value) -> Self {
        Self {
            tag,
            value,
            quality: Quality::Good,
        }
    }
}

/// 协议私有计划的类型擦除容器。
///
/// 不变量：只有产出该计划的驱动可以 `downcast_ref`；类型不符由驱动返回
/// `DriverError::Fatal`，不会影响其他设备。
pub struct PlanPayload(Arc<dyn Any + Send + Sync>);

impl PlanPayload {
    pub fn new<T: Any + Send + Sync>(payload: T) -> Self {
        Self(Arc::new(payload))
    }

    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref::<T>()
    }
}

impl Clone for PlanPayload {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl fmt::Debug for PlanPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PlanPayload(<opaque>)")
    }
}

/// 一个轮询组：周期 + 该组产出的 tag + 驱动私有执行载荷。
#[derive(Clone)]
pub struct PollGroup {
    pub name: String,
    pub interval: Duration,
    /// 该组产出的全部 tag（含位展开子 tag），用于通信中断时批量置 `comm_lost`
    pub tags: Vec<TagId>,
    pub payload: PlanPayload,
}

impl fmt::Debug for PollGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PollGroup")
            .field("name", &self.name)
            .field("interval", &self.interval)
            .field("tags", &self.tags.len())
            .finish()
    }
}

/// 一个设备编译后的轮询计划。
#[derive(Clone, Debug)]
pub struct DevicePlan {
    pub groups: Vec<PollGroup>,
    /// 编译期间收集的非致命问题（未知覆盖键等）
    pub warnings: Vec<String>,
}

impl DevicePlan {
    pub fn all_tags(&self) -> Vec<TagId> {
        let mut tags = Vec::new();
        for g in &self.groups {
            tags.extend(g.tags.iter().cloned());
        }
        tags
    }
}

/// 编译输入。
pub struct CompileInput<'a> {
    pub device_id: &'a str,
    pub overrides: &'a serde_json::Map<String, Json>,
    pub points: &'a PointsTable,
    pub protocol: &'a ProtocolDecl,
}

/// 协议驱动端口。
///
/// 实现放在独立 crate（如 `hmi-driver-modbus`），由宿主注册到 gateway。
/// 驱动必须可跨通道共享（同一实例服务多台设备），所有设备状态放在
/// [`DevicePlan`]/[`PollGroup`] 里，不得放在驱动自身。
#[async_trait]
pub trait Driver: Send + Sync {
    /// 驱动 id（设备包 `protocol.driver` 与之匹配）
    fn id(&self) -> &'static str;

    /// 显示名
    fn name(&self) -> &'static str {
        self.id()
    }

    /// 该驱动支持的传输种类
    fn transports(&self) -> &'static [TransportKind];

    /// 校验设备包（安装/导入时调用）。默认不做额外校验。
    fn validate_package(&self, _points: &PointsTable, _protocol: &ProtocolDecl) -> DriverIssues {
        DriverIssues::default()
    }

    /// 校验设备实例地址；默认接受任意值。
    fn validate_address(&self, _address: &DeviceAddress) -> Result<(), String> {
        Ok(())
    }

    /// 把点表编译成协议私有计划。
    fn compile(&self, input: &CompileInput<'_>) -> Result<DevicePlan, DriverError>;

    /// 执行一个轮询组。
    async fn poll(
        &self,
        link: &dyn Link,
        address: &DeviceAddress,
        group: &PollGroup,
        timeout: Duration,
    ) -> Result<Vec<Sample>, DriverError>;

    /// 写一个点（`value` 已是工程量）。
    ///
    /// `protocol` 用于解析驱动私有参数（如 Modbus 的 framing/byteOrder）。
    async fn write(
        &self,
        link: &dyn Link,
        address: &DeviceAddress,
        point: &PointDef,
        value: &Value,
        protocol: &ProtocolDecl,
        timeout: Duration,
    ) -> Result<(), DriverError>;

    /// 探测设备是否在线（`Ok(())` = 在线）。
    ///
    /// `protocol` 用于解析 framing；扫描无设备包场景可传默认声明。
    async fn probe(
        &self,
        link: &dyn Link,
        address: &DeviceAddress,
        protocol: &ProtocolDecl,
        timeout: Duration,
    ) -> Result<(), DriverError>;
}
