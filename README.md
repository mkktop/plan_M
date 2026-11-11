# 上位机平台（plan_M）

可扩展、高自定义的工业上位机。核心思路：**声明式设备包 + 双进程架构 + 订阅制实时数据**。

第一轮已交付「采集核心 + gatewayd」，无 UI 可独立运行与验收；桌面壳（Tauri + Vue 3）为下一轮。

## 架构

```
┌─ UI 进程（下一轮：Tauri 2 + Vue 3，随时可崩可关）─────┐
│    配置页 / 实时点表 / 仪表盘 / UI 插件                  │
└──────────────┬──────────────────────────────────────┘
               │ WebSocket @127.0.0.1:59120（JSON 消息，token 鉴权）
┌──────────────┴──────────────┐
│ gatewayd 采集进程（常驻）     │
│ ├─ 通道监督器 × N（串口）     │
│ ├─ 轮询编译器（读块合并）     │
│ ├─ TagHub 实时库（订阅分发）  │
│ ├─ 设备包注册表（声明式）     │
│ └─ axum：/health + /ws       │
└───┬─────────┬───────────────┘
 COM3      COM5     …（每条 485 总线一个通道，挂 N 个从站）
```

设计纪律：

- **domain / gateway / packaging 不依赖 tauri / 串口实现**。总线打开由 `BusOpener` 注入，
  驱动通过 `Driver` 端口注入 —— gatewayd 负责接线。将来拆独立网关部署零重构。
- **驱动跟随设备包声明**（`protocol.driver`），一条总线允许混协议设备。
- **质量戳从第一天跟随每个值**：通信中断 → `comm_lost`（保留最后好值），UI 据此变灰而不是显示假数据。

## 目录

```
crates/
├─ domain/            纯模型：Tag/Quality/Modbus 地址工程记法/字节序编解码/点表/配置/端口 trait
├─ driver-modbus-rtu/ Modbus RTU 驱动：帧编解码、串口传输、虚拟总线 + 内存从站（SimSlave）
├─ packaging/         设备包：manifest/点表解析、语义校验、安装（目录/zip）、注册表
├─ gateway/           运行时：轮询编译器、通道监督器（串行事务/重试/离线/重连）、TagHub、服务编排
└─ wire/              gatewayd ↔ UI 的 WS 消息契约（serde，两端共用）
apps/
└─ gatewayd/          CLI：serve / poll / scan / pkg
examples/packages/
└─ pm3000/            示例设备包（三相电表，覆盖 u16/u32/i32/cdab/BCD/位展开/可写点/双轮询组）
```

## 快速开始

```bash
cargo build --workspace
cargo test --workspace          # 81 个测试：编解码/帧/虚拟总线端到端/包校验/WS 协议

# 1) 校验示例设备包
gatewayd pkg validate examples/packages/pm3000

# 2) 无硬件轮询（虚拟从站 + 预置演示数据）
gatewayd poll --package examples/packages/pm3000 --com SIM --once

# 3) 启动服务（--sim = 演示模式：自动装示例包 + SIM 总线 + 两台演示设备）
gatewayd serve --sim
#   → http://127.0.0.1:59120/health
#   → ws://127.0.0.1:59120/ws           （token 见启动输出 / <data-dir>/gateway.token）
#   → 数据目录：%APPDATA%/gatewayd（--data-dir 可覆盖）

# 4) 真硬件
gatewayd scan --com COM3 --baud 9600            # 扫总线从站
gatewayd poll --package examples/packages/pm3000 --com COM3 --slave 1
```

`serve --sim` 期间任何 WS 客户端可用如下消息流验证（Node 一行即可）：

```js
ws.send(JSON.stringify({type:'hello', token}));
ws.send(JSON.stringify({type:'getSnapshot'}));
ws.send(JSON.stringify({type:'subscribe', tags:['pm1.uab','pm1.status.b0']}));
// → {type:'changes', items:[{tag:'pm1.uab', value:{f64:220.1}, quality:'good', tsMs:...}]}
```

## 设备包（声明式，导入即增加设备）

一个目录（或 zip）= `manifest.json` + `points.json`：

```json
// manifest.json
{
  "schemaVersion": 1,
  "id": "com.acme.pm3000",
  "name": "ACME PM3000 三相电表",
  "version": "1.2.0",
  "minHostVersion": "0.1.0",
  "protocol": { "driver": "modbus-rtu", "defaults": { "byteOrder": "cdab", "respTimeoutMs": 150 } }
}
```

```json
// points.json（节选）
{
  "pollGroups": [
    { "name": "fast", "intervalMs": 1000 },
    { "name": "slow", "intervalMs": 60000 }
  ],
  "points": [
    { "id": "uab", "name": "AB线电压", "register": "40101", "type": "u16",
      "scale": 0.1, "unit": "V", "pollGroup": "fast" },
    { "id": "energy", "name": "有功电度", "register": "40200", "type": "u32",
      "byteOrder": "cdab", "scale": 0.01, "unit": "kWh", "pollGroup": "slow" },
    { "id": "status", "register": "40300", "type": "u16",
      "bits": { "0": { "name": "运行" }, "2": { "name": "报警" } } },
    { "id": "setpoint", "register": "40310", "type": "u16", "access": "rw" }
  ]
}
```

要点：

- **地址用工程记法**（与设备手册口径一致）：`40101` 保持寄存器、`30001` 输入寄存器、
  `00010` 线圈、`10010` 离散量；也支持显式 `HR:100` / `IR:5` / `C:3` / `DR:7`。
- **字节序显式声明**：`abcd`（标准）/ `cdab`（国产电表最常见字交换）/
  `badc`（字内字节交换）/ `dcba`（全反转）。多字值不写会 warning —— 485 头号坑。
- **位展开**：状态字加 `bits` 后自动生成 `<点>.b0` 子 tag（只读）。
- **轮询组**：按频率分组，编译器把组内相邻寄存器**合并成读块**（间隔 ≤4 合并，≤125 字/块），
  30 点的设备通常只需 3~4 个事务。
- 实例覆盖：设备实例可用 `overrides` 单点改 `scale`/`offset`/`pollGroup`（同型号多台不复制点表）。
- 编辑器支持：`crates/packaging/schema/*.schema.json` 可直接给 VS Code 做补全校验。

## WS 协议（摘要，完整见 `crates/wire`）

客户端 → 服务端：`hello{token}` / `getSnapshot` / `subscribe{tags}` / `unsubscribe` /
`writeTag{tag,value}` / `scanChannel{channel,range}` / `getSerialPorts` / `getPackages` /
`upsertChannel` / `removeChannel` / `addDevice` / `removeDevice` / `importPackage{path}`

服务端 → 客户端：`welcome` / `snapshot` / `changes{items}`（10~20ms 合并批量）/
`channelState` / `scanProgress` / `scanDone` / `serialPorts` / `packages` / `commandOk` / `error`

## 下一轮（UI 与插件）

1. Tauri 2 壳（Vue 3 + Vite）：串口/通道/设备配置页、实时点表（订阅制，只看当前画面）
2. 设备包导入界面（zip 拖拽 → 校验错误精确到字段）
3. 内置仪表盘（画布 + widget 网格）
4. UI 插件机制（ESM 动态加载 + `PluginCtx` 门面：tags/alarms/i18n/theme）
5. 报警引擎 + 历史归档（SQLite WAL）
6. rhai 脚本驱动（私有协议，沙箱内跑）

## 已知边界（本轮刻意不做）

- 无 TLS（本地回环服务，token 鉴权已够用；跨机部署时再加）
- 历史/报警未落库（SQLite 已留好位置）
- `scanChannel` 在 WS 会话内同步执行（几十秒级，UI 做进度条时再改异步）
