# 架构：四层协议模型

本项目的核心目标是「设备包声明式扩展 + 任意传输/协议可插拔」。为避免 485/Modbus 语义
渗入采集主干，代码被拆成四层，依赖方向严格向下：

```
设备包 (protocol + points)
    │
ChannelConfig (transport: serial | tcp | sim | ble + timing)
    │
TransportFactory ──open──> Link
    │
Driver (compile/poll/write/probe)
    │
Gateway（调度 / 重试 / 质量戳 / TagHub）
```

## 1. 分层职责

| 层 | 代码 | 职责 | 不知道什么 |
|---|---|---|---|
| Transport | `transport-serial` / `transport-tcp` / `transport-sim` | 打开连接、字节收发、目标发现（串口枚举） | 不知道寄存器、CRC、MBAP |
| Link | `crates/domain/src/link.rs` | 已打开连接的 `send/recv/drain` | 不知道协议 |
| Driver | `driver-modbus` 等 | 校验设备包、编译点表、执行轮询/写点/探测 | 不知道通道来自串口还是 TCP（只读 `link.kind()` 选帧） |
| Gateway | `crates/gateway` | 按组调度、重试/离线/重连、质量戳、TagHub、配置/包生命周期 | 不认识任何 Modbus/串口类型 |

`domain` 只放契约与纯函数（不依赖 tokio/tauri/axum）。`gateway` 与 `packaging`
不依赖任何具体驱动；宿主 `apps/gatewayd` 负责注册驱动与传输。

## 2. 关键契约

### Link

```rust
trait Link {
    fn kind(&self) -> TransportKind;
    async fn send(&self, bytes: &[u8]) -> Result<(), LinkError>;
    async fn recv(&self, max: usize, timeout: Duration) -> Result<Vec<u8>, LinkError>;
    async fn drain(&self);
    fn as_any(&self) -> &dyn Any; // BLE GATT 等非字节协议的逃生口
}
```

`recv` 返回 `1..=max` 字节（流式语义）；协议层用 `domain::link::read_exact` 按帧凑齐。
超时清残帧由 `drain` 完成。

### TransportFactory

```rust
trait TransportFactory {
    fn kind(&self) -> TransportKind;
    async fn open(&self, channel: &ChannelConfig) -> Result<Box<dyn Link>, TransportError>;
    async fn targets(&self) -> Vec<TransportTarget>;
}
```

`open` 是异步的（TCP connect / BLE 扫描）；重连由 supervisor 反复调用。

### Driver

```rust
trait Driver {
    fn id(&self) -> &'static str;
    fn transports(&self) -> &'static [TransportKind];
    fn validate_package(&self, points: &PointsTable, protocol: &ProtocolDecl) -> DriverIssues;
    fn validate_address(&self, address: &DeviceAddress) -> Result<(), String>;
    fn compile(&self, input: &CompileInput) -> Result<DevicePlan, DriverError>;
    async fn poll(&self, link, address, group, timeout) -> Result<Vec<Sample>, DriverError>;
    async fn write(&self, link, address, point, value, protocol, timeout) -> Result<(), DriverError>;
    async fn probe(&self, link, address, protocol, timeout) -> Result<(), DriverError>;
}
```

- `compile` 把点表与实例覆盖编译成 `PollGroup`（周期 + tags + 驱动私有 `PlanPayload`）。
  读块合并、位展开、字节序解析全部在驱动内。
- `poll` 只执行一个组；成功返回 `Sample { tag, value, quality }`，由 gateway 写入 TagHub。
- 驱动必须无设备状态：所有状态放在 plan 里，驱动实例可跨通道共享。

### 错误分类

`DriverError::poll_class()`：

| 分类 | 触发 | supervisor 行为 |
|---|---|---|
| `LinkDead` | `LinkError::Closed/Io` | 整通道 `recovering`，退避重连 |
| `Retry` | 应答超时、从站异常、帧损坏 | 重试 N 次，耗尽标记设备离线 |
| `Fatal` | 非法地址、不可恢复异常 | 直接标记设备离线，周期探测 |

## 3. 配置 v2

`config.json`：

```json
{
  "channels": [
    {
      "id": "ch1", "name": "现场总线", "transport": "serial",
      "timing": { "respTimeoutMs": 200, "retries": 2, "probeIntervalMs": 5000 },
      "port": "COM3", "baudRate": 9600, "dataBits": 8, "parity": "none",
      "stopBits": 1, "interFrameGapMs": 5
    },
    {
      "id": "tcp1", "name": "WiFi 网关", "transport": "tcp",
      "timing": { "respTimeoutMs": 300, "retries": 1, "probeIntervalMs": 5000 },
      "host": "192.168.1.10", "port": 502, "connectTimeoutMs": 3000
    }
  ],
  "devices": [
    { "id": "pm1", "channelId": "ch1", "package": "com.acme.pm3000", "address": 1 },
    { "id": "pm2", "channelId": "tcp1", "package": "com.acme.pm3000", "address": 1 }
  ]
}
```

设备地址是**驱动私有**的 `DeviceAddress`（`number | string`）。Modbus 用 1–247；
BLE 可用 MAC；其他协议可用任意稳定标识。

## 4. 设备包 v2

```json
{
  "schemaVersion": 2,
  "protocol": {
    "driver": "modbus",
    "transports": ["serial", "tcp", "sim"],
    "params": { "framing": "auto", "byteOrder": "abcd" }
  }
}
```

- `transports` 缺省 = 驱动的全部传输；安装时校验「包声明 ⊆ 驱动能力」。
- `params` 由驱动解释。Modbus 支持 `framing: auto|rtu|tcp`
  （`auto` = 串口/SIM 用 RTU、TCP 用 MBAP；`rtu` 可支持 TCP 转 485 网关）。
- 点地址：`"address": "40101"` 或对象 `{ "ble": { ... } }`。字符串只接受寄存器记法。

## 5. 运行时数据流

```
config.json
  └─ GatewayService::new（扫包 + 建驱动/传输注册表）
     └─ restart_channel
        ├─ 选 TransportFactory（按 channel.transport）
        ├─ 每台设备：查包 → 查驱动 → 兼容性校验 → driver.compile
        └─ spawn_channel
           ├─ factory.open(channel) ──> Link
           ├─ 按设备×组调度
           │   └─ driver.poll(link, address, group)
           │       └─ Sample[] → TagHub → WS changes
           └─ 错误分类 → 重试 / 设备离线 / 通道重连
```

写点：`GatewayService::write_tag` 做通用校验（access/min/max/step/enum）→
`driver.write` 自行编码（Modbus 驱动按寄存器/线圈与字节序编码）。

## 6. 认证与安全边界

- WS 首条消息 `hello{token}`，token 来自 `<data-dir>/gateway.token`。
- 设备包安装时校验 driver 已注册、transports 兼容、驱动私有规则；
  ESM 组件入口与 UI 插件资源只暴露声明路径，防目录穿越。
- 当前只监听 `127.0.0.1`，无 TLS（跨机部署时再加）。
