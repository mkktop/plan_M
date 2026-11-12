# 扩展指南：新增传输 / 协议

本项目的扩展点只有两个：`TransportFactory`（传输）与 `Driver`（协议）。
两者都在 `crates/domain` 定义，宿主 `apps/gatewayd/src/lib.rs` 注册。
gateway / supervisor / packaging / wire 主干不需要改动。

## A. 新增一种传输（例如 BLE）

### 1. 配置类型

在 `crates/domain/src/config.rs`：

- `TransportKind` 增加枚举值（本项目已预留 `Ble`）。
- `TransportConfig` 增加变体与参数字段（已预留 `BleParams { adapter, address }`）。

旧配置无需迁移时可直接改；若已有工程数据，新增字段必须带 `#[serde(default)]`。

### 2. 新建 crate

```
crates/transport-ble/
  Cargo.toml
  src/lib.rs
```

实现：

```rust
pub struct BleFactory;

#[async_trait]
impl TransportFactory for BleFactory {
    fn kind(&self) -> TransportKind { TransportKind::Ble }
    async fn open(&self, channel: &ChannelConfig) -> Result<Box<dyn Link>, TransportError> {
        // 发现/连接/配对/MTU 协商都在这里
        let conn = BleConnection::connect(...).await?;
        Ok(Box::new(conn))
    }
    async fn targets(&self) -> Vec<TransportTarget> { /* 扫描到的外设 */ }
}

#[async_trait]
impl Link for BleConnection {
    fn kind(&self) -> TransportKind { TransportKind::Ble }
    async fn send(&self, bytes: &[u8]) -> Result<(), LinkError> { /* GATT write */ }
    async fn recv(&self, max: usize, timeout: Duration) -> Result<Vec<u8>, LinkError> { /* notify 队列 */ }
    async fn drain(&self) { /* 清 notify 队列 */ }
    fn as_any(&self) -> &dyn Any { self }
}
```

注意：

- BLE 断线必须映射为 `LinkError::Closed`，supervisor 会自动重连。
- 透传模块（UART service）按字节流实现即可；原生 GATT 传感器可以通过
  `Link::as_any()` 暴露 GATT 句柄，由专用驱动下转型。
- 不要在传输层解析任何协议帧。

### 3. 注册

`apps/gatewayd/src/lib.rs`：

```rust
pub fn transports() -> Vec<Arc<dyn TransportFactory>> {
    vec![
        Arc::new(hmi_transport_serial::SerialFactory),
        Arc::new(hmi_transport_tcp::TcpFactory),
        Arc::new(hmi_transport_ble::BleFactory),   // 新增
        simnet::sim_factory(),
    ]
}
```

### 4. UI

- `Welcome` 自动带 `transports` 能力；通道表单只显示宿主上报的传输。
- 需要在 `ChannelFormModal.vue` 增加该传输的字段组（BLE 已有 `adapter/address` 预留）。
- `ChannelConfig` / `ChannelDto` 是判别联合，新增 `transport` 后 TS 会提示所有需要补的 switch。

### 5. 测试

- 至少覆盖：打开失败、发送/接收、超时、断线映射。
- 若能在 CI 中用内存/本地回环模拟，优先做（参考 `transport-tcp` 的 loopback 测试）。

## B. 新增一种协议

### 1. 新建 crate

```
crates/driver-xxx/
  Cargo.toml
  src/lib.rs
```

实现 `Driver`：

```rust
pub struct XxxDriver;

#[async_trait]
impl Driver for XxxDriver {
    fn id(&self) -> &'static str { "xxx" }
    fn transports(&self) -> &'static [TransportKind] { &[TransportKind::Serial, TransportKind::Tcp] }

    fn validate_package(&self, points: &PointsTable, protocol: &ProtocolDecl) -> DriverIssues {
        // 解析 protocol.params，检查点地址形状/类型/可写性
    }

    fn validate_address(&self, address: &DeviceAddress) -> Result<(), String> { ... }

    fn compile(&self, input: &CompileInput) -> Result<DevicePlan, DriverError> {
        // 点表 + overrides → 分组 → 每组的协议私有 PlanPayload
    }

    async fn poll(&self, link, address, group, timeout) -> Result<Vec<Sample>, DriverError> {
        // 用 group.payload.downcast_ref::<XxxGroupPlan>() 取计划
    }

    async fn write(...) -> Result<(), DriverError> { ... }
    async fn probe(...) -> Result<(), DriverError> { ... }
}
```

### 2. 设备包

```json
{
  "schemaVersion": 2,
  "protocol": {
    "driver": "xxx",
    "transports": ["serial", "tcp"],
    "params": { /* 驱动私有默认值 */ }
  }
}
```

点地址用对象形式：

```json
{ "id": "temp", "address": { "xxx": { "path": "/temp", "type": "i16" } } }
```

### 3. 注册

`apps/gatewayd/src/lib.rs`：

```rust
pub fn drivers() -> Vec<Arc<dyn Driver>> {
    vec![
        Arc::new(hmi_driver_modbus::ModbusDriver::new()),
        Arc::new(hmi_driver_xxx::XxxDriver::new()),   // 新增
    ]
}
```

### 4. 约定

- 驱动必须无设备状态；计划放在 `PollGroup::payload`。
- `poll` 成功返回本组全部 tag 的 `Sample`；失败返回正确分类的 `DriverError`。
- 写值由驱动编码，gateway 只做通用 access/min/max/step/enum 校验。
- `validate_package` 在安装时执行，未知驱动/不兼容传输会被拒绝。
- 设备包安装后，`PackageDto.protocol` 会下发给 UI；非 Modbus 驱动只需隐藏 Modbus 专属列。

## C. 蓝牙的两条落地路径

### C1. 透传模块（最常见）

- 新增 `transport-ble`（字节流 `Link`）。
- 复用 `driver-modbus`，设备包 `transports` 加 `"ble"`，`params.framing = "rtu"`。
- 设备地址仍是 Modbus 从站号。

### C2. 原生 GATT 传感器

- `transport-ble` 暴露 GATT 句柄（`Link::as_any()`）。
- 新增 `driver-ble-gatt`：`compile` 把点地址（service/characteristic/format）
  编成读/通知计划；`poll` 读特征值或消费通知；`write` 写特征值。
- gateway / packaging / wire 主干无需改动；UI 的 `transports` 能力会自动出现 BLE，
  通道表单与设备地址输入已支持文本地址。

## D. 检查清单

新增传输：

- [ ] `TransportKind` + `TransportConfig` 变体（serde default 兼容）
- [ ] `TransportFactory` / `Link` 实现（超时/断线语义正确）
- [ ] 宿主注册
- [ ] UI 字段组 + 能力过滤
- [ ] 至少一个端到端测试

新增协议：

- [ ] `Driver` 全部方法实现，`poll_class` 语义正确
- [ ] `validate_package` / `validate_address`
- [ ] 设备包 v2 示例
- [ ] 宿主注册
- [ ] 一个真实或模拟链路端到端测试
- [ ] 文档中标注点地址形状与 `params`
