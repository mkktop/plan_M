# 路线图与交接文档（Roadmap / Handoff）

> 本文件用于跨会话交接：新会话从这里恢复上下文即可继续开发。
> 最后更新：2026-11-11（第一轮完成时）。

## 0. 一句话背景

可扩展、高自定义的上位机：**Tauri 2 + Rust 双进程**架构。`gatewayd` 是常驻采集进程
（串口/轮询/实时库/设备包/WS 服务），桌面 UI 是它的一个 WebSocket 客户端。
设备能力通过**声明式设备包**扩展（导入即增加设备），UI 能力将通过 **ESM UI 插件**扩展。

## 1. 当前状态（第一轮已完成并提交）

- Rust 1.99.1（MSVC）已装；Node 24 + pnpm 11 + WebView2 + npm 版 tauri CLI 2.11.2 就绪。
- Workspace：`crates/domain`、`crates/driver-modbus-rtu`、`crates/packaging`、
  `crates/gateway`、`crates/wire`、`apps/gatewayd`。
- **81 个测试全绿**；clippy 0 警告；fmt 通过。
- 实机验收通过：`pkg validate` / `poll --com SIM` / `serve --sim` + `/health` +
  独立 WebSocket 客户端全协议（hello→snapshot→subscribe→changes、错误 token 拒绝）。
- 示例设备包 `examples/packages/pm3000`（13 点，覆盖 u16/u32/i32/cdab/位展开/可写/双轮询组）。

### 常用命令

```bash
cargo build --workspace
cargo test --workspace
cargo run -p hmi-gatewayd -- serve --sim          # 演示模式（自动装示例包 + SIM 总线 + pm1/pm2）
cargo run -p hmi-gatewayd -- poll --package examples/packages/pm3000 --com SIM --once
cargo run -p hmi-gatewayd -- scan --com COM3
cargo run -p hmi-gatewayd -- pkg validate examples/packages/pm3000
```

- 服务地址：`http://127.0.0.1:59120/health`、`ws://127.0.0.1:59120/ws`
- 数据目录默认 `%APPDATA%/gatewayd`：`config.json`、`gateway.token`、`packages/`、`logs/`
- SIM 演示数据在 `apps/gatewayd/src/simnet.rs`（寄存器预置值，改这里可改演示现象）

### 已确定且不要推翻的设计决策

1. **双进程**：UI 可崩可关，采集常驻。`domain/gateway/packaging` 不允许依赖 tauri 或串口实现
   （总线打开由 `BusOpener` 注入、驱动从 `Driver` 端口注入，见 `apps/gatewayd/src/lib.rs`）。
2. **驱动跟随设备包**（`manifest.protocol.driver`），一条 485 总线允许混协议设备。
3. **485 语义**：一条总线单任务串行事务；单事务超时→重试→从站离线→5s 周期探测恢复；
   串口 I/O 错→整通道 `recovering` 退避重连；离线期间 tag 质量 `comm_lost`（保留最后好值）。
4. **质量戳**（good/bad/uncertain/comm_lost）必须贯穿所有数据路径。
5. **订阅制**：UI 只订阅当前画面 tag；`changes` 10~20ms 合并批量推。
6. **设备包 = 声明式 JSON**（地址用设备手册工程记法如 `40101`；多字类型必须显式 byteOrder）。
   校验用 serde 强类型 + 语义规则（`crates/packaging/src/validate.rs`），另有 JSON Schema
   文件供编辑器补全（`crates/packaging/schema/`）。
7. **前端框架已定**：Vue 3 + Vite + TypeScript + Pinia。

### 关键契约速查

- WS 消息：`crates/wire/src/lib.rs`（serde 判别字段 `type`，camelCase）。
  `ClientMsg`: hello/getSnapshot/subscribe/unsubscribe/writeTag/scanChannel/getSerialPorts/
  getPackages/upsertChannel/removeChannel/addDevice/removeDevice/importPackage
  `ServerMsg`: welcome/snapshot/changes/channelState/scanProgress/scanDone/serialPorts/
  packages/commandOk/error
- **Value 的 JSON 形状是外部标签枚举**：`{"f64":220.1}` / `{"i64":300}` / `{"bool":true}` /
  `{"str":"..."}`；`TagChange = {tag, value, quality, tsMs}`。
- 设备包格式：`README.md` 的「设备包」一节 + `examples/packages/pm3000/`。
- 轮询编译：`crates/gateway/src/poll.rs`（间隔≤4 合并、≤125 字/块）。
- 通道监督器：`crates/gateway/src/supervisor.rs`；实时库：`crates/gateway/src/hub.rs`。

### 已知边界（有意为之）

- 无 TLS（本地回环 + token）；`scanChannel` 在 WS 会话内同步执行（几十秒级）；
- 历史/报警未落库；配置是 JSON（SQLite 后路已留 `ConfigStore` trait）；
- 数据目录 `data/demo`（本地调试产物）已在 `.gitignore`。

## 2. 第二轮：Tauri 2 桌面壳 + Vue 3 基础 UI（下一轮，优先）

### 目标与验收

`pnpm tauri dev` 启动桌面应用 → 自动/手动连上 `gatewayd serve --sim` →
看到 2 台演示电表的实时点表（1s 刷新、质量色、可写点可写）、可管理通道/设备、可扫描串口。
断线/重连有状态提示。

### 任务清单

1. **脚手架**
   - `apps/desktop/src-tauri/`（Tauri 2，thin：不让 UI 层写业务逻辑）
   - `apps/desktop/ui/`（Vue 3 + TS + Vite；`pnpm` 管理；Element Plus 或 Naive UI 二选一）
   - Cargo workspace 增加 `apps/desktop/src-tauri`；Tauri 依赖只在此 crate。
2. **Rust 侧（src-tauri）**
   - Tauri command `gateway_info() -> {wsUrl, token, hostVersion}`：读 `%APPDATA%/gatewayd/gateway.token`
     （或 `--data-dir` 配置），端口默认 59120。
   - 健康检查 `GET /health`；未启动时提示启动命令；后续再加 sidecar 自动拉起
     （Tauri `externalBin` 打 `gatewayd.exe`，dev 阶段先手动跑）。
   - 退出策略：UI 退出不杀 gatewayd（采集常驻）；托盘/自动启动留到第六轮。
3. **前端基础设施**
   - `ui/src/api/ws.ts`：单例 WS 客户端 —— 重连（指数退避）、hello/token、请求/响应匹配、
     订阅集去重（同一 tag 多组件订阅只发一次）、`changes` 分发到 Pinia。
   - Pinia stores：`connection`（状态/版本/错误）、`project`（channels/devices/packages 快照）、
     `tags`（Map<tagId, TagChange>，含 quality/tsMs）。
   - 组件：状态栏（连接状态/版本）、质量色规范（good=绿、bad=黄、comm_lost=灰、stale 闪烁）。
4. **页面（按顺序）**
   - 诊断页：连接配置（ws 地址/token）、health、快照统计、重连按钮。
   - 通道页：`getSerialPorts` 列表（含 hwId）、新建/编辑通道（波特率/校验/超时/重试）、
     通道状态（running/recovering）、从站扫描（`scanChannel`，结果列表）。
   - 设备页：已安装包下拉 → 选从站地址 → `addDevice`；设备卡显示在线/离线（`channelState`
     事件 + snapshot.online）；删除设备。
   - 实时点表：按设备分组展示 tag（名称来自 packages.points 映射）、值按类型格式化
     （f64 收敛尾差、bool 开关文案）、只订阅可见行、写值对话框（仅 `access=rw`）。
5. **验收脚本**：`gatewayd serve --sim` 跑着 → UI 各页操作不报错；拔掉/恢复（用
   `simnet` 改 online 或重启 gatewayd）→ 质量色与重连提示正确。

### 注意事项

- WS 地址/端口/token 要集中配置（不散落）；token 文件不存在时给出明确提示。
- 前端不实现协议拼装逻辑之外的东西；所有业务真值以 gatewayd 为准。
- 值格式化工具函数放 `ui/src/utils/format.ts`（f64 最多 6 位小数去尾零）。

## 3. 第三轮：设备包导入 + 仪表盘

- **设备包导入**：拖拽 zip → `importPackage{path}`（gatewayd 本机路径）。
  Tauri 侧文件对话框拿到绝对路径；导入后展示校验错误（`ServerMsg::error` 需带可读原因，
  必要时把 `PackageError` 序列化进 error 文案）。包列表页展示点表预览（名称/地址/单位）。
- **CSV 点表导入**（可选加分）：Excel 手册点表 → CSV → 生成 points.json（前端做转换，
  或 gatewanyd 加 `pkg import-csv` 命令）。
- **内置仪表盘**：网格画布 + 内置 widget（数值卡、开关按钮、简易仪表盘）。
  - 布局持久化：新增 wire 消息 `getDashboards/saveDashboard`，gatewayd 存
    `<data-dir>/dashboards.json`（UI 可重装，布局属于工程）。
  - 组件契约按未来插件模型设计（`{ id, type, config }`），内置 widget 与未来第三方 widget 同构。
- **验收**：导入新包（改个 id/点表）→ 添加设备 → 仪表盘拖 widget 绑点 → 重启 gatewayd/UI 后布局还在。

## 4. 第四轮：UI 插件机制（ESM）

- **宿主**：gatewayd 静态服务 `GET /plugins/{id}/*`（axum 从 `<data-dir>/ui-plugins/` 读，
  带 CORS、版本查询串缓存失效）；Tauri CSP 放行 `http://127.0.0.1:59120`。
- **插件形态**：目录 = `manifest.json` + 构建好的 ESM `index.js` + 资源。
  manifest 贡献点：`widgets`（仪表盘组件）、`pages`（导航页）、`devicePanels`（设备详情面板）；
  `permissions: ["tags:subscribe","tags:write", ...]`；`sdkVersion` 校验。
- **SDK**：`ui/packages/ui-plugin-sdk`（TS 类型 + ctx 工厂）。
  `PluginCtx`：`tags.subscribe/read/write`、`alarms.subscribe`（第五轮）、`i18n`、`theme`、
  `notify`、`device`（devicePanels 注入）、`log`；入口 `export function mount(el, ctx)`。
- **订阅复用**：插件不自己开 WS；宿主按 tagId 聚合去重后统一订阅并分发。
- **安装/开发**：UI 上传 zip → gatewayd 解压校验；dev 模式网关配置开发目录 + no-cache。
- **安全 v1**：同 webview + 门面 + manifest 权限校验（写点必须声明）；v2 再上 iframe 沙箱。
- **示例插件**：`examples/ui-plugins/meter-card`（无框架纯 JS，证明框架无关）。
- **验收**：装 meter-card → 仪表盘可选该 widget → 删除插件后画布显示占位不崩。

## 5. 第五轮：报警 + 历史

- **报警**：点级限值（设备包 `alarms` 默认 + 实例覆盖），状态机 active/ack/cleared；
  `ServerMsg::alarms` 推送 + 快照；UI 报警条/报警页/确认操作。
- **历史**：gatewayd 内 SQLite（WAL）+ 批量写（按变化/最小间隔）；表 `tag_history(tag, ts, value, quality)`；
  wire 增加 `getHistory{tags, from, to, maxPoints}`；保留策略（按天/容量）。
- **UI**：ECharts 历史曲线页；报警历史列表。
- **验收**：设限值触发报警（SIM 改值）→ 确认/恢复流转正确；历史查询能画出曲线。

## 6. 第六轮：rhai 脚本驱动 + 打包分发

- **rhai 驱动**：设备包可带 `driver.rhai`（`protocol.driver = "script"`），钩子
  `encode_read/encode_write/decode`；宿主白名单 API（crc16/hex/位运算），预编译校验；
  `Driver` 端口复用现有调度。
- **打包**：Tauri `externalBin` sidecar 打 `gatewayd.exe`；安装包/托盘常驻/开机自启
  （可选 Windows 服务模式）；headless 网关部署文档。
- **验收**：用 rhai 包跑通一台非标协议设备的 poll；安装包在干净机器上开箱可用。

## 7. 新会话开场建议

1. 读本文件 + `README.md`，然后 `cargo test --workspace` 确认基线绿。
2. 需要看 UI 效果时先跑 `cargo run -p hmi-gatewayd -- serve --sim`，用 Node/临时页面验证 WS。
3. 第二轮从「脚手架 + ws.ts + 诊断页」开始，先把连接链路打通再做业务页。
4. 新增 wire 消息要同时改 `crates/wire` 与 gatewayd 的 `serve.rs`，并补 ws_smoke 断言。
5. 保持 crate 边界：`domain/gateway/packaging` 不引入 tauri 依赖。
