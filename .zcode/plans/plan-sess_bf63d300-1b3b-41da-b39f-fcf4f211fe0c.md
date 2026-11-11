# 上位机平台 · 第一轮实施：采集核心 + gatewayd

## Context

全新项目（`D:\work_for_mkk\plan_M` 为空目录）。目标架构已在对话中确定：**Tauri + Rust，双进程**——`gatewayd` 常驻采集进程（串口/轮询/TagHub/设备包/WS 服务，纯 Rust 无 Tauri 依赖）+ Tauri 桌面 UI（下一轮做，前端已定 Vue 3 + Vite）。本轮交付**完整采集链路**：Modbus RTU 驱动、通道调度、实时库、设备包解析校验、gatewayd 可执行程序（serve + poll/scan/pkg 命令），全部带测试。本轮不含任何 UI。

环境现状：Node/pnpm/WebView2/Tauri CLI 就绪；**Rust 工具链未安装**（第一步解决）。

## 总体结构（本轮产出）

```
plan_M/
├─ Cargo.toml                  # workspace
├─ crates/
│  ├─ domain/                  # 纯模型：Tag/Quality/地址/字节序解码 —— 无 tokio、无 tauri
│  ├─ wire/                    # WS 消息类型（serde，JSON over WebSocket）
│  ├─ gateway/                 # 通道调度器/轮询编译/TagHub/服务编排
│  ├─ driver-modbus-rtu/       # 第一个驱动（drivers/ 目录下）
│  └─ packaging/               # 设备包 manifest+点表 解析/校验/安装/注册表
├─ apps/
│  └─ gatewayd/                # 可执行：serve / poll / scan / pkg 子命令
├─ examples/packages/pm3000/   # 示例设备包（三相电表）
└─ .gitignore
```

## 实施步骤

### Step 0：安装 Rust 工具链（前置条件）
- 下载 rustup-init.exe，装 stable-x86_64-pc-windows-msvc
- 用 vswhere 检查 VS Build Tools / Windows SDK：有 link.exe 就跳过；没有则 winget 安装 Build Tools C++ 工作负载（下载量大，装前告知）
- 验证：`cargo new` + `cargo build` 一次 hello world

### Step 1：workspace 骨架 + git init
空 crate 骨架、公共依赖版本对齐（tokio/serde/thiserror/tracing/anyhow/semver/jsonschema/clap/axum(ws feature)/serialport+tokio-serial/dashmap/zip/dirs/async-trait/futures、dev: tokio-tungstenite）。

### Step 2：crates/domain（纯模型 + 解码）
- `Value`(F64/I64/Bool/String)、`Quality`(Good/Bad/CommLost/Uncertain)、`TagSnapshot`、`TagId`
- Modbus 地址：`RegArea`(Holding/Input/Coil/Discrete) + 0 基偏移；**解析工程记法 "40101"**（电表手册口径），错误记法报具体原因
- `DataType`(U16/I16/U32/I32/F64/F32/U64/Bcd16/Bcd32) + `ByteOrder`(ABCD/DCBA/BADC/CDAB)，`decode_registers(data_type, byte_order, words) -> Value`、`encode_value(...)`（写路径）
- BCD 解码（电表重灾区）、状态字 `bits` 展开模型
- 单元测试：地址解析、字节序×类型矩阵、BCD golden 用例

### Step 3：crates/driver-modbus-rtu
- `BusTransport` trait：`transact(request, timeout) -> Vec<u8>`（一问一答的独占总线语义）。两个实现：`SerialTransport`（tokio-serial）+ `MockTransport`（测试/无硬件验证）
- RTU 帧编解码：0x01/0x03/0x04/0x06/0x10，CRC16-MODBUS，异常码解析，帧完整性校验
- `ModbusRtuDriver`：**接口收"读块"（area/start/count），不做点合并**——合并归 gateway 的轮询编译器，脚本驱动将来复用同一抽象
- 测试：golden 帧字节比对；MockTransport + 内存 Modbus slave 应答器的回环测试

### Step 4：crates/gateway（运行时核心）
- `Driver` trait（async）：`read_blocks(bus, slave, blocks)` / `write_point(...)`
- `PollCompiler`：设备点表 + 实例 overlay（scale/pollGroup 覆盖）→ 按 pollGroup 分组 → **地址间隔 ≤4 的寄存器合并成读块**（485 带宽优化核心）
- `ChannelSupervisor`：每通道一个 tokio 任务；事务严格串行（半双工）；超时（默认 200ms）重试 2 次后标记从站离线、跳过继续轮询他人；串口打开失败/消失 → 重连状态机（指数退避封顶 5s），期间该通道全部 Tag 置 `CommLost`；支持按 VID/PID 硬件 ID 匹配 COM 号漂移
- `TagHub`：`DashMap<TagId, TagSnapshot>` 最新值缓存 + 每订阅者 mpsc、按订阅集过滤的增量分发
- `GatewayService`：持有通道/设备/包注册表，启停通道，暴露命令（写点、扫描从站、枚举串口）与变更流
- 配置存储 v1：JSON 工程文件（data_dir/config.json），`ConfigStore` trait 留 SQLite 后路
- 集成测试：MockTransport 虚拟总线 → PollCompiler → 轮询 → TagHub 断言值与 quality 流转（good→超时→CommLost→恢复）

### Step 5：crates/packaging（设备包）
- `PackageManifest` / `PointsFile` serde 类型 + `schemaVersion`
- jsonschema 校验（schema 内嵌为文本），报错带行号信息；semver `minHostVersion` 检查
- 安装流：解压/拷贝到临时目录 → 校验 → 原子移入 `data_dir/packages/<id>/<version>/`；`PackageRegistry` 启动扫描目录重建注册（目录为真相源）
- 点表默认值继承（manifest protocol.defaults.byteOrder 等）

### Step 6：crates/wire（IPC 契约）
- `ClientMsg`：GetSnapshot / Subscribe / Unsubscribe / WriteTag / ScanChannel / GetSerialPorts / UpsertChannel / RemoveChannel / AddDevice / ImportPackage…
- `ServerMsg`：Snapshot / Changes(Vec<TagChange> 批量) / Alarms / ChannelState / ScanResult / Error
- serde JSON over WebSocket，两端（未来的 UI）共用类型

### Step 7：apps/gatewayd（可执行）
clap 子命令：
- `gatewayd serve [--port 59120] [--data-dir X]`：axum，`GET /health`、`GET /ws`（首条消息 token 握手鉴权），消息循环对接 GatewayService
- `gatewayd poll --package <dir> --com COM3 --slave 1 [--baud 9600] [--once|持续]`：直接轮询打印点表（无硬件调试神器，gwtest 并入）
- `gatewayd scan --com COM3 --range 1-247`：扫总线从站
- `gatewayd pkg validate <dir>` / `gatewayd pkg install <zip|dir>`
- tracing 日志：stdout + 按天滚动文件（data_dir/logs）

### Step 8：示例设备包 + 收尾
- `examples/packages/pm3000/`：对话中定稿的 PM3000 三相电表包（manifest + points.json + icon 占位）
- rustfmt、README（架构图、命令用法、如何接真设备）
- `git init` + 首次提交

## 验证

1. `cargo build --workspace`、`cargo test --workspace` 全绿（覆盖：地址/字节序/BCD、Modbus golden 帧、虚拟总线端到端集成、包校验/安装）
2. `gatewayd pkg validate examples/packages/pm3000` → 通过；故意改坏点表 → 精确报错
3. `gatewayd serve` 启动 → `curl /health` 通；用 tokio-tungstenite 写的 WS 冒烟测试完成 GetSnapshot→Subscribe→收到 Changes 全流程（MockTransport 通道）
4. 真硬件（可选，需你提供 COM 口）：`gatewayd scan` 找到从站 → `gatewayd poll` 读出实际值

## 不在本轮范围

Tauri 桌面壳、仪表盘、UI 插件机制、报警引擎、历史归档（SQLite）、rhai 脚本驱动——trait 和 wire 消息已为其预留位置，下一轮接 Vue 3 前端。