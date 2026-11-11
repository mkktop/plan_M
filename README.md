# 上位机平台（plan_M）

可扩展、高自定义的工业上位机。核心思路：**声明式设备包 + 双进程架构 + 订阅制实时数据**。

已交付「采集核心 + gatewayd」「Tauri 2 + Vue 3 桌面 UI」与「设备包导入 + 仪表盘」。
采集进程可独立常驻运行，桌面壳只是它的 WebSocket 客户端；UI 崩溃/关闭不影响采集。

## 架构

```
┌─ UI 进程（Tauri 2 + Vue 3，随时可崩可关）──────────────┐
│    诊断 / 仪表盘 / 通道 / 设备 / 设备包 / 实时点表        │
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
├─ gateway/           运行时：轮询编译器、通道监督器（串行事务/重试/离线/重连）、TagHub、服务编排、
│                     仪表盘布局存储（dashboards.json）
└─ wire/              gatewayd ↔ UI 的 WS 消息契约（serde，两端共用）
apps/
├─ gatewayd/          CLI：serve / poll / scan / pkg
└─ desktop/
   ├─ src-tauri/      Tauri 2 薄壳（gateway_info 读取 token、dialog 插件；不写业务逻辑）
   └─ ui/             Vue 3 + TS + Vite + Pinia（WS 客户端 / 六个业务页）
examples/packages/
└─ pm3000/            示例设备包（三相电表，覆盖 u16/u32/i32/cdab/BCD/位展开/可写点/双轮询组）
```

## 快速开始

```bash
cargo build --workspace
cargo test --workspace          # 89 个测试：编解码/帧/虚拟总线端到端/包校验/仪表盘存储/WS 协议

# 1) 校验示例设备包
gatewayd pkg validate examples/packages/pm3000

# 2) 无硬件轮询（虚拟从站 + 预置演示数据）
gatewayd poll --package examples/packages/pm3000 --com SIM --once

# 3) 启动服务（--sim = 演示模式：自动装示例包 + SIM 总线 + 两台演示设备）
gatewayd serve --sim
#   → http://127.0.0.1:59120/health
#   → ws://127.0.0.1:59120/ws           （token 见启动输出 / <data-dir>/gateway.token）
#   → 数据目录：%APPDATA%/gatewayd（--data-dir 可覆盖）
#      config.json / gateway.token / packages/ / dashboards.json / logs/

# 4) 桌面 UI（先保持 serve --sim 运行）
cd apps/desktop
pnpm install                # 首次
pnpm tauri dev              # 自动拉起 vite + Tauri 窗口

# 浏览器调试模式（不启动 Tauri，另开终端）
pnpm --dir ui dev           # http://localhost:1420/?token=<gateway.token 内容>

# 5) 真硬件
gatewayd scan --com COM3 --baud 9600            # 扫总线从站
gatewayd poll --package examples/packages/pm3000 --com COM3 --slave 1
```

Tauri 桌面壳会自动读取 `<data-dir>/gateway.token` 与默认端口 59120；浏览器调试模式用
`?token=…&ws=ws://127.0.0.1:59120/ws` 覆盖。改连接设置会存 localStorage。

`serve --sim` 期间任何 WS 客户端可用如下消息流验证（Node 一行即可）：

```js
ws.send(JSON.stringify({type:'hello', token}));
ws.send(JSON.stringify({type:'getSnapshot'}));
ws.send(JSON.stringify({type:'subscribe', tags:['pm1.uab','pm1.status.b0']}));
// → {type:'changes', items:[{tag:'pm1.uab', value:{f64:220.1}, quality:'good', tsMs:...}]}
```

## 桌面 UI（第二、三轮）

Vue 3 + TypeScript + Vite + Pinia；Naive UI 只做表单控件，外壳/表格/弹窗为自绘，便于后续换肤与插件化。

- **诊断**：连接状态、WS 地址/token、`/health` 探测与延迟、运行统计、完整通信日志（收发帧摘要）。
- **仪表盘**：12 列网格画布；数值卡 / 开关 / 仪表三种内置组件，绑定任意设备 tag；编辑模式拖拽移动、
  右下角缩放（不允许重叠），改动防抖自动保存到 gatewayd（`<data-dir>/dashboards.json`）；开关只对
  `access=rw` 的寄存器/线圈点可操作，值来自订阅回读而非本地猜测。
- **通道**：串口枚举（含硬件 ID）、通道卡片与运行状态、新建/编辑（波特率/校验/超时/重试/帧间隔）、
  从站扫描（结果可直接跳转添加设备）。
- **设备**：按通道分组、在线状态、设备包版本与点位数、添加/删除设备、从设备包页带着型号预填添加。
- **设备包**：已安装包列表、点表预览（地址/类型/单位/换算/读写/位展开）、在用设备数；
  桌面版支持 **zip/目录拖放** 与文件对话框，浏览器调试模式用 gatewayd 本机路径；导入失败逐字段内联展示，
  成功后自动选中新包并可直接添加设备。
- **实时点表**：按设备查看点位；只订阅当前设备的 tag（bit 子 tag 一并订阅），值/单位/质量/更新时间，
  状态字位展开为芯片，`access=rw` 的点可写值（工程量，gatewayd 负责反算 scale/offset）。

基础设施：`ui/src/api/ws.ts` 单例客户端负责 hello/token 握手、指数退避重连、请求响应配对、
**订阅引用计数去重**、重连后自动重放订阅；Pinia 分为 `connection/project/tags/dashboards/toast`。
质量色规范：good 绿、bad 黄、uncertain 蓝、comm_lost 灰（保留最后好值，UI 不显示假数据）。
Tauri 侧只加了一个 `tauri-plugin-dialog`（文件/目录选择）与 `gateway_info`，业务真值仍在 gatewayd。

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
`upsertChannel` / `removeChannel` / `addDevice` / `removeDevice` / `importPackage{path}` /
`getDashboards` / `saveDashboard{dashboard}` / `removeDashboard{id}`

服务端 → 客户端：`welcome` / `snapshot` / `changes{items}`（10~20ms 合并批量）/
`channelState` / `deviceState` / `scanProgress` / `scanDone` / `serialPorts` / `packages` /
`dashboards` / `commandOk` / `error`

仪表盘布局（`DashboardDto`）：`{ id, name, cols, widgets[] }`；widget 为
`{ id, kind, title, tag, unit, config, layout{x,y,w,h} }`。`config` 由组件自行解释
（内置 value=`decimals`、switch=`onLabel/offLabel`、gauge=`min/max`），未来插件 widget 同构扩展。
gatewayd 保存时校验 id/边界/**组件不重叠**，布局属工程数据，UI 可重装。

## 下一轮（UI 插件机制）

完整计划、验收标准与交接说明见 **[docs/ROADMAP.md](docs/ROADMAP.md)**。概要：

1. UI 插件 ESM 动态加载（gatewayd 静态服务 + `PluginCtx` 门面 + manifest 权限）
2. 报警引擎 + 历史归档（SQLite WAL）
3. rhai 脚本驱动（私有协议，沙箱内跑）+ 打包分发

## 已知边界（刻意不做）

- 无 TLS（本地回环服务，token 鉴权已够用；跨机部署时再加）
- Tauri 尚未自动拉起 gatewayd（sidecar 在后续轮次）；开发期先手动 `serve`
- 浏览器调试模式导入设备包需要 gatewayd 本机路径；拖放/文件对话框只在桌面壳内可用
- 仪表盘布局是「最后写入者胜」，多人同时编辑同一看板不做冲突合并
- 历史/报警未落库（SQLite 已留好位置）
- `scanChannel` 在 WS 会话内同步执行（几十秒级；扫描期间该连接的其他请求会排队）
- Tauri `csp` 暂为 null（插件机制落地时再收紧）
