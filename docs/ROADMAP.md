# 路线图与交接文档（Roadmap / Handoff）

> 本文件用于跨会话交接：新会话从这里恢复上下文即可继续开发。
> 最后更新：2026-11-11（第五轮完成时）。

## 0. 一句话背景

可扩展、高自定义的上位机：**Tauri 2 + Rust 双进程**架构。`gatewayd` 是常驻采集进程
（串口/轮询/实时库/设备包/工作区/UI 插件/WS 服务），桌面 UI 是它的一个 WebSocket 客户端。
设备能力通过**声明式设备包**扩展（导入即增加设备），UI 能力通过 **ESM UI 插件**扩展。

## 1. 当前状态（第一～五轮已完成）

- Rust 1.99.1（MSVC）已装；Node 24 + pnpm 11 + WebView2 + Tauri CLI 2.12.4 就绪。
- Workspace：`crates/domain`、`crates/driver-modbus-rtu`、`crates/packaging`、
  `crates/gateway`、`crates/wire`、`apps/gatewayd`、`apps/desktop/src-tauri`。
- 后端 **103 个测试全绿**；clippy 0 警告；fmt 通过；`vue-tsc` + `vite build` 通过。
- 实机验收通过：`pkg validate` / `poll --com SIM` / `serve --sim` + `/health` +
  独立 WebSocket 客户端全协议；桌面 UI（浏览器模式）走完 诊断→通道→设备→实时点表→写值→
  扫描→添加/删除设备→断线重连全流程。
- 示例设备包 `examples/packages/pm3000`（13 点，覆盖 u16/u32/i32/cdab/位展开/可写/双轮询组）。
- **第二轮 UI 已交付**：`apps/desktop/ui`（Vue 3 + Vite + Pinia + Naive UI 表单）与
  `apps/desktop/src-tauri`（Tauri 2 薄壳，`gateway_info` 读 token）。四个页面：
  诊断 / 通道 / 设备 / 实时点表；订阅引用计数去重、指数退避重连、质量色、写值（工程量）。
- **第三轮已交付**：设备包页（点表预览 / zip 拖放与文件对话框 / 错误内联 / 包→添加设备预填）+
  仪表盘（12 列网格、数值卡/开关/仪表、拖动缩放、防抖自动保存、gatewayd `dashboards.json` 持久化）。
  验收走通：导入坏包看逐字段错误 → 导入新包 → 添加设备 → 建看板 → 三种组件 → 开关写值回读 →
  拖动/缩放 → 刷新页面与重启 gatewayd 后布局仍在。
- **第四轮已交付**：ESM UI 插件机制。
  - 后端：`crates/packaging/src/ui_plugin.rs`（manifest/权限/entry/贡献点校验、目录/zip 安装、
    目录扫描注册表，5 个单测）；`GatewayService` 插件列表/导入/移除/真实目录查询；
    gatewayd `GET /plugins` + `GET /plugins/<id>/<entry>`（CORS、no-cache、路径越界防护）。
  - wire：`UiPluginDto` + `getUiPlugins/importUiPlugin/removeUiPlugin` + `uiPlugins` 应答；ws_smoke 覆盖安装/删除。
  - 前端：`plugins/host.ts`（动态 import、权限门面、插件 tag 聚合、ctx 生命周期）、
    `PluginsPage`（列表/导入/拖放/错误内联/移除）、`PluginPage` + 侧栏动态导航、
    仪表盘插件组件渲染（`plugin:<插件id>:<组件id>`）与插件 config JSON 编辑。
  - 示例：`examples/ui-plugins/meter-card`（零依赖 ESM，组件 + 导航页）；SDK 类型
    `apps/desktop/ui-plugin-sdk/index.d.ts`。
  - Tauri：启用生产/开发 CSP，放行 `127.0.0.1:59120` 的插件脚本、连接与图片。
  - 验收实测：插件页导入示例 → 入口已加载、导航页出现 → 仪表盘选插件组件绑定 `pm1.uab` →
    渲染 220.1V/GOOD/量程 180–260 → 布局落 `dashboards.json` → 刷新页面与重启 gatewayd 后仍在 →
    移除插件后组件退化为占位符、导航项消失 → 重新导入恢复；坏插件导入显示逐字段错误；
    Tauri 窗口内（WebView2 + CSP）插件资源成功加载并渲染 220.1V。
- **第五轮已交付**：设备类型工作区。
  - 设备包 manifest 新增可选 `ui.pages`（旧包不升 schemaVersion，缺省走宿主 fallback 四页）；
    宿主组件白名单 `overview / dashboard / tags / settings`；校验 page/component。
  - wire：`PackageUiDto`、`DevicePageDto`、`WorkspaceDto` + `getWorkspaces/saveWorkspace/removeWorkspace`；
    `DashboardDto.scopeKind/scopeId`；`DeviceDto.overrides` 回填设置页。
  - gateway：`workspaces.json` 存储（tmp+rename、损坏容错、保存校验，4 个单测）；package DTO 映射 ui；
    dashboard scope 校验。
  - UI：顶部类型标签（一个设备包一个类型，导入即出现）；`/t/:typeId/:pageId?` 工作区；
    同类型共享页面 + 实例下拉；左侧页面导航 + 底部固定「系统」入口；
    `DeviceOverviewPanel` / `DeviceSettingsPanel`；dashboard/tags 嵌入模式（按设备隔离）；
    页面编辑器（页面/组件增删排序、config JSON、恢复包默认）。
  - 验收实测：类型标签/四页/实例切换/点表隔离/设备改名/按设备看板/自定义页面保存与刷新直链/
    恢复默认/无 ui 旧包 fallback/无设备类型空状态/系统入口切换全部通过。

### 常用命令

```bash
cargo build --workspace
cargo test --workspace
cargo run -p hmi-gatewayd -- serve --sim          # 演示模式（自动装示例包 + SIM 总线 + pm1/pm2）
cargo run -p hmi-gatewayd -- poll --package examples/packages/pm3000 --com SIM --once
cargo run -p hmi-gatewayd -- scan --com COM3
cargo run -p hmi-gatewayd -- pkg validate examples/packages/pm3000

# 桌面 UI（先保持 serve --sim 运行）
cd apps/desktop && pnpm install
pnpm tauri dev                                    # Tauri 窗口（token 自动读 %APPDATA%/gatewayd）
pnpm --dir ui dev                                 # 浏览器调试：?token=<gateway.token>
```

- UI 目录：`apps/desktop/ui/src/{api,stores,pages,components,components/device,styles,plugins,utils}`；
  `api/ws.ts` 是协议客户端单例，`api/bridge.ts` 是 WS→Pinia 唯一分发点，
  `plugins/host.ts` 是 ESM 插件宿主，`utils/devicePages.ts` 是设备页面组件目录/fallback。
  页面：`HomePage / DiagnosticsPage / DashboardPage / ChannelsPage / DevicesPage / PackagesPage /
  PluginsPage / PluginPage / TagsPage / DeviceWorkspacePage`；
  设备工作区路由 `/t/:typeId/:pageId?`；根路径 `/` 进入 Hello 首页。
  `styles/pages.css` 放设备包/仪表盘/插件/首页/工作区专用样式（基础设计系统仍为 `theme.css`）。

- 服务地址：`http://127.0.0.1:59120/health`、`ws://127.0.0.1:59120/ws`、
  插件清单 `http://127.0.0.1:59120/plugins`
- 数据目录默认 `%APPDATA%/gatewayd`：`config.json`、`gateway.token`、`packages/`、
  `ui-plugins/`、`workspaces.json`、`dashboards.json`、`logs/`
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
7. **前端已定**：Vue 3 + Vite + TypeScript + Pinia + Naive UI（仅表单）；
   外壳/表格/弹窗自绘。单例 WS 客户端 + 订阅引用计数，页面只读 Pinia。

### 关键契约速查

- WS 消息：`crates/wire/src/lib.rs`（serde 判别字段 `type`，camelCase）。
  `ClientMsg`: hello/getSnapshot/subscribe/unsubscribe/writeTag/scanChannel/getSerialPorts/
  getPackages/upsertChannel/removeChannel/addDevice/removeDevice/importPackage/
  getDashboards/saveDashboard/removeDashboard/getUiPlugins/importUiPlugin/removeUiPlugin/
  getWorkspaces/saveWorkspace/removeWorkspace
  `ServerMsg`: welcome/snapshot/changes/channelState/deviceState/scanProgress/scanDone/
  serialPorts/packages/dashboards/uiPlugins/workspaces/commandOk/error
- **Value 的 JSON 形状是外部标签枚举**：`{"f64":220.1}` / `{"i64":300}` / `{"bool":true}` /
  `{"str":"..."}`；`TagChange = {tag, value, quality, tsMs}`；quality 字符串
  `good|bad|uncertain|comm_lost`。
- 快照 DTO 已含 UI 所需字段：`ChannelDto` 带完整串口参数；`PointDto` 带
  `register/area/type/access/scale/offset/unit/bits[]`。设备上下线是独立的
  `deviceState` 事件（不再复用 `channelState`）。
- 仪表盘 DTO：`DashboardDto { id, name, cols, scopeKind?, scopeId?, widgets[] }`；
  `WidgetDto { id, kind, title, tag, unit, config, layout{x,y,w,h} }`。
  `config` 为组件私有 JSON（value=`decimals`、switch=`onLabel/offLabel`、gauge=`min/max`），
  插件 widget 复用同一形状。`scopeKind/scopeId` 缺省 = 全局看板，`device` = 设备工作区看板。
  保存时 gatewayd 校验：id 规则、边界、组件不重叠、scope 成对且合法。
  `dashboards` 消息是 get/save/remove 的统一应答（返回完整列表）。
- 设备包 UI：`PackageDto.ui.pages[]`，page 为
  `{ id, title, icon, components[{ kind, title, config }] }`；kind 白名单
  `overview|dashboard|tags|settings`（`ALLOWED_DEVICE_PAGE_COMPONENTS`）。
  manifest 里同形状（`ui.pages`）；旧包缺省时 UI fallback 四页。
- 工作区 DTO：`WorkspaceDto { deviceType, pages[] }`（deviceType = 包 id）；
  `workspaces` 是 get/save/remove 的统一应答。保存到 `<data-dir>/workspaces.json`，
  校验页面 id/标题/组件白名单/数量。没有记录时使用包默认页面。
- 设备工作区路由：`/t/:typeId/:pageId?`，实例选择用 `?device=<id>`；
  顶部类型标签来自已安装包；左侧设备模式显示页面、系统模式显示全局导航，底部固定「系统」入口。
- `DeviceDto.overrides` 回填参数设置页；保存设置复用 `addDevice`（同 id upsert）。
- UI 插件 DTO：`UiPluginDto { id, name, version, description, author, sdkVersion, permissions[],
  entry, widgets[], pages[], devicePanels[] }`；`uiPlugins` 是 get/import/remove 的统一应答。
  manifest 校验：id/version/sdkVersion、entry 相对路径且存在、权限白名单
  (`tags:subscribe|tags:write|notify|theme`)、贡献点 id 唯一/尺寸 1–12。
  安装到 `<data-dir>/ui-plugins/<id>/`，目录为真相源；资源路由
  `GET /plugins/<id>/<entry>`（CORS + no-cache + canonicalize 越界防护）。
- 插件组件 kind：`plugin:<插件id>:<组件id>`；`WidgetDto.config` 原样透传，由插件解释。
  插件导航页路由 `/plugin/<插件id>/<页面id>`，侧栏由 `pluginHost.pages` 动态生成。
- 插件入口 `export function mount(el, ctx, props)`，返回清理函数；`props.kind` 为 `widget` 或
  `page`。`ctx.tags.subscribe/read/write`、`ctx.notify`、`ctx.theme`、`ctx.log`；
  权限未声明的调用抛错/拒绝。插件不自己开 WebSocket，宿主按 tag 聚合去重并重放订阅。
- Tauri CSP：生产和开发都放行 `127.0.0.1:59120`（script/connect/img）；dev 额外放行
  `localhost:1420` 与 HMR WebSocket。改 CSP 后必须用 `pnpm tauri dev` 实测插件能加载。
- Tauri 命令 `gateway_info`：见 `apps/desktop/src-tauri/src/lib.rs`；
  文件/目录选择走 `tauri-plugin-dialog`（capability `dialog:default` + JS `@tauri-apps/plugin-dialog`）。
- UI 连接默认值：`ws://127.0.0.1:59120/ws`；浏览器调试用 `?token=&ws=` 覆盖。
- 设备包格式：`README.md` 的「设备包」一节 + `examples/packages/pm3000/`。
- 轮询编译：`crates/gateway/src/poll.rs`（间隔≤4 合并、≤125 字/块）。
- 通道监督器：`crates/gateway/src/supervisor.rs`；实时库：`crates/gateway/src/hub.rs`。

### 已知边界（有意为之）

- 无 TLS（本地回环 + token）；`scanChannel` 在 WS 会话内同步执行（几十秒级）；
- 历史/报警未落库；配置是 JSON（SQLite 后路已留 `ConfigStore` trait）；
- 仪表盘布局 last-write-wins（多人同时编辑不合并）；损坏的 `dashboards.json` 按空布局启动并告警；
- 设备类型 = 设备包；同类型共享页面配置，但仪表盘按设备实例独立保存；
- 用户保存的工作区页面不会随设备包升级自动合并（恢复默认可刷新）；损坏的 `workspaces.json`
  按无覆盖启动并告警；
- 历史/报警未落库；配置是 JSON（SQLite 后路已留 `ConfigStore` trait）；
- UI 插件 v1 与宿主同 webview，权限是能力门面不是强沙箱；`devicePanels` 已解析但不渲染；
  插件导航页目前属于系统工作区，设备类型专属插件页/面板留到后续；
  插件入口按 `version` 查询串缓存，改文件不升版本时需要刷新页面或重启 gatewayd；
- 数据目录 `data/`（本地调试产物）已在 `.gitignore`。

## 2. 第二轮：Tauri 2 桌面壳 + Vue 3 基础 UI（已完成）

### 实施结果（2026-11-11）

- 目录：`apps/desktop/{package.json,pnpm-workspace.yaml,src-tauri/,ui/}`；
  pnpm workspace 管理 `@hmi/ui`，`pnpm tauri dev` 自动拉起 Vite（1420）。
- Tauri 薄壳：`gateway_info` 命令读 `<data-dir>/gateway.token`（默认 `%APPDATA%/gatewayd`，
  端口 59120），返回 `{wsUrl,token,tokenPath,dataDir,port,hostVersion}`；无业务逻辑。
  窗口 1440×900（最小 1120×720）、深色标题栏；图标由 `app-icon.svg` 生成。
- 前端：`api/ws.ts`（握手/重连/请求配对/订阅 refcount/日志环）、`api/bridge.ts`（消息→Pinia）、
  stores `connection/project/tags/toast`；页面 诊断/通道/设备/实时点表；自绘卡片/表格/弹窗/Toast，
  Naive UI 只用于表单控件。
- 验收：`serve --sim` + 浏览器模式走通：连接→通道列表/串口枚举→扫描 1-3→从扫描结果预填添加设备→
  写 setpoint 250→300 并回读→删除设备→kill/重启 gatewayd 自动重连并恢复订阅。
- 与原始计划的差异：
  1. UI 库选 **Naive UI**（表单控件）+ 自绘外壳，未用 Element Plus；
  2. 设备包导入先做「gatewayd 本机路径」版本（zip 拖拽/dialog 留到第三轮）；
  3. Tauri sidecar 自动拉起 gatewayd 未做（第六轮），开发期手动 `serve`；
  4. `csp` 暂为 null（本地回环 + token；插件机制落地时再收紧）。

### 目标与验收（原始计划，留档）

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
- **Volar/vue-tsc 已知坑**：模板静态属性值里出现字面量 `ws://…` 会破坏后续表达式的
  ref 解包（报 `Property 'value' does not exist`）。静态 placeholder 用中文描述，
  或把 URL 放脚本常量里绑定。
- 点表窄屏：`theme.css` 有 1500/1180 两档断点；加列后先用 1280 宽检查是否裁切。

## 3. 第三轮：设备包导入 + 仪表盘（已完成）

### 实施结果（2026-11-11）

- **设备包页**（`PackagesPage.vue`）：左列表 + 右详情；详情含厂商/版本/驱动/描述、
  点位数与位展开数、可写点数、在用设备数，以及逐点点表（工程地址、类型、单位/换算、读写、位定义）。
  - 导入器：桌面端 **Tauri 原生拖放**（`onDragDropEvent`）与 **zip/目录文件对话框**
    （`tauri-plugin-dialog`）；浏览器调试模式保留绝对路径输入并明确提示。
  - 错误内联展示：`ServerMsg::error` 原文（含 `manifest.json 解析失败（行/列）`、语义校验列表）
    显示在弹窗内 `<pre>`，不再只弹一条 toast。
  - 导入成功后按「新 id / 版本变化」选中新包；「用此包添加设备」跳到设备页并预填型号。
- **仪表盘**（`DashboardPage.vue`）：12 列网格、行高 64px；内置 `value/switch/gauge` 三类组件。
  - 编辑模式：拖动移动、右下角缩放、组件设置（类型/标题/绑定 tag/单位、value 小数位、
    gauge 量程、switch 文案）；碰撞检测，**组件不允许重叠**。
  - 数据：只订阅当前看板绑定的 tag；开关写值走 `writeTag` 并由订阅回读，只允许 `access=rw`
    寄存器/线圈点（位子 tag 只读）。
  - 持久化：改动 600ms 防抖保存，显式串行化请求避免并发覆盖；状态栏显示「保存中/已保存/保存失败」。
  - 多看板：新建/切换/重命名/删除；布局真值在 gatewayd。
- **后端**：`crates/gateway/src/dashboards.rs`（`DashboardStore`，tmp+rename 原子落盘、
  损坏文件告警后空布局启动、id/边界/重叠校验，4 个单测）；`GatewayService` 暴露
  `dashboards/save_dashboard/remove_dashboard`；`serve.rs` 处理 3 条新消息；
  `ws_smoke` 增加「保存 → 列表应答 → 删除」断言。
- **Tauri**：`tauri-plugin-dialog` + capability `dialog:default`；仍然只有 `gateway_info`
  一个业务外命令，采集/布局真值全在 gatewayd。
- **验收实测**：坏 zip → 行 1 列 3 解析错误内联；新包 `com.acme.pm3001` 导入 → 自动选中 →
  带着型号添加 `pm3001-3`（SIM 从站 3 在线）→ 新建看板 → 数值卡 220.1V、开关
  setpoint ON→OFF（写值回读）、仪表 50Hz（量程 45–65）→ 拖拽/缩放后 `dashboards.json`
  记录新坐标 → 刷新页面与重启 gatewayd 后 3 个组件与实时值仍在；Tauri 窗口启动后
  `netstat` 可见 `msedgewebview2` 到 59120 的 ESTABLISHED WS。

### 原始计划（留档）

- **设备包导入**：拖拽 zip → `importPackage{path}`（gatewayd 本机路径）。
  Tauri 侧文件对话框拿到绝对路径；导入后展示校验错误（`ServerMsg::error` 需带可读原因）。
  包列表页展示点表预览（名称/地址/单位）。
- **CSV 点表导入**（可选加分）：Excel 手册点表 → CSV → 生成 points.json。
- **内置仪表盘**：网格画布 + 内置 widget（数值卡、开关按钮、简易仪表盘）。
  - 布局持久化：wire 消息 `getDashboards/saveDashboard`，gatewayd 存
    `<data-dir>/dashboards.json`（UI 可重装，布局属于工程）。
  - 组件契约按未来插件模型设计（`{ id, kind, config }`），内置与未来第三方 widget 同构。
- **验收**：导入新包（改个 id/点表）→ 添加设备 → 仪表盘拖 widget 绑点 → 重启 gatewayd/UI 后布局还在。

### 与原始计划的差异

1. **CSV 点表导入未做**（可选加分项，留在后续按真实手册样本再做）；
2. 布局保存为 last-write-wins，未做多客户端并发合并；
3. 布局校验在保存时执行，加载旧版非法文件不自动迁移（只告警/由 UI 拖回合法位置）；
4. 仪表盘组件为手写 SVG/CSS（未引入图表库），保持零额外依赖。

## 4. 第四轮：UI 插件机制（ESM）（已完成）

### 实施结果（2026-11-11）

- **后端包层**：`crates/packaging/src/ui_plugin.rs`
  - `UiPluginManifest`（schemaVersion/id/version/sdkVersion/entry/permissions/widgets/pages/devicePanels）；
  - 校验：id 规则、semver、sdkVersion ≤ 1、entry 相对路径且文件存在（.js/.mjs）、权限白名单、
    贡献点 id 唯一、默认尺寸 1–12；
  - 安装：目录/zip → 临时目录 → 校验 → `<data-dir>/ui-plugins/<id>/`（同 id 覆盖）；
  - 注册表：目录扫描，损坏插件记录 `broken` 不阻塞服务。5 个单测（目录/zip/非法 manifest/损坏/升级）。
- **gateway/gatewayd**：`GatewayService::{ui_plugins, ui_plugin_dir, import_ui_plugin, remove_ui_plugin}`；
  `GET /plugins`（JSON 清单）与 `GET /plugins/{id}/{*path}` 静态资源
  （CORS、`no-cache`、canonicalize 越界防护、真实安装目录查找）。
  WS 新增 `getUiPlugins/importUiPlugin/removeUiPlugin` → `uiPlugins` 应答；ws_smoke 覆盖安装/删除。
- **前端宿主**：`ui/src/plugins/host.ts`
  - 动态 `import()` gatewayd 的 ESM 入口（URL 带 `?v=<version>`）；
  - `PluginContext`：`tags.subscribe/read/write`、`notify`、`theme`、`log`，权限未声明直接拒绝；
  - 插件订阅聚合：所有插件共用一条 `gateway.onMessage` + 宿主引用计数订阅，重连自动重放；
  - 注册表索引：`plugin:<id>:<widget>` → 组件、`/plugin/<id>/<page>` → 导航页；加载失败记录错误。
- **UI 页面**：`PluginsPage`（列表/权限/贡献点/加载状态、zip/目录导入、拖放、错误内联、移除确认）、
  `PluginPage`（动态页面宿主）、`App.vue` 侧栏动态导航、仪表盘插件组件渲染与 config JSON 编辑。
  插件删除/加载失败时组件显示占位符、导航项消失，不崩溃。
- **示例与 SDK**：`examples/ui-plugins/meter-card`（纯 ESM 零依赖：仪表卡片 + 导航页 + notify 演示）；
  `apps/desktop/ui-plugin-sdk/index.d.ts`（PluginContext/manifest 类型）。
- **Tauri CSP**：`csp`/`devCsp` 放行 `127.0.0.1:59120` 的脚本、连接、图片；dev 额外放行 Vite。
  实测 WebView2 中插件资源加载成功并渲染实时值。
- **验收实测**：导入示例（目录）→ 入口已加载/导航页出现 → 仪表盘选插件组件绑 `pm1.uab` →
  220.1V/GOOD/量程 180–260 → 布局写入 `dashboards.json` → 刷新页面/重启 gatewayd 后仍在 →
  移除插件 → 占位符且导航消失 → 重新导入恢复；坏插件导入显示权限与 entry 两条错误；
  HTTP：`/plugins` 清单、资源 `text/javascript` + CORS、路径穿越 400、缺失资源 404。

### 原始计划（留档）

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

### 与原始计划的差异

1. 安装入口用 **gatewayd 本机路径**（目录/zip）而不是浏览器上传文件；与设备包导入一致，
   桌面端由 Tauri dialog 提供绝对路径。浏览器调试模式仍需手动填路径。
2. SDK 放在 `apps/desktop/ui-plugin-sdk`（类型定义，未走 pnpm workspace 构建）；
   `ctx.i18n`/`ctx.device`/`alarms.subscribe` 未实现（alarms 随第五轮；devicePanels 暂不渲染）。
3. 插件入口 URL 只按 `version` 做缓存失效；开发期改了插件代码但不升版本时需刷新页面。
4. 权限门面在宿主代码里强制执行，但插件与宿主同 webview，v1 不是强沙箱。

## 5. 第五轮：设备类型工作区（已完成）

### 实施结果（2026-11-11）

- **packaging**：`PackageManifest.ui`（可选 `ui.pages`）、`PackageUiDecl/UiPageDecl/UiComponentDecl`；
  校验 page id/标题/组件白名单/数量/配置；`manifest.schema.json` 补 ui；pm3000 示例声明四页。
- **wire**：`PackageUiDto/DevicePageDto/DevicePageComponentDto`；`WorkspaceDto`；
  `getWorkspaces/saveWorkspace/removeWorkspace` + `workspaces` 应答；
  `DashboardDto.scopeKind/scopeId`；`DeviceDto.overrides`（设置页回填）。
- **gateway**：`workspaces.json` 存储（tmp+rename、损坏容错、保存校验，4 个单测）；
  `GatewayService` 工作区读写；`package_dto` 映射 ui；dashboard scope 成对校验。
- **UI**：
  - `DeviceTypeTabs`：顶部类型标签来自已安装设备包，导入即出现；无包时提示导入。
  - `DeviceWorkspacePage`：`/t/:typeId/:pageId?`，同类型页面共享，实例下拉 `?device=`；
    无设备类型显示添加引导；无包显示缺失状态；直链/刷新后按 workspace 恢复页面。
  - `DeviceOverviewPanel`：订阅当前设备 tag，展示在线/质量统计/通道与包信息/需要关注的 tag。
  - `DeviceSettingsPanel`：名称/通道/从站/overrides 编辑，复用 `addDevice` upsert。
  - `DevicePagesEditor`：页面增删/改名/图标/排序；页内组件增删/排序/kind/config JSON；
    保存到 workspaces.json；恢复包默认页面。
  - `DashboardPage`/`TagsPage` 增加 embedded + scope/device props；看板按设备隔离，点表只显示当前设备。
  - `App.vue`：设备模式/系统模式左栏切换，底部固定「系统」入口，顶部标题动态。
- **验收实测**：类型标签/四页/实例切换/点表隔离/设备改名/按设备看板/自定义页面保存与刷新直链/
  恢复默认/无 ui 旧包 fallback/无设备类型空状态/系统入口切换全部通过。

### 原始计划（留档）

- 顶部标签 = 设备类型；导入设备包即出现；一个包 = 一个类型。
- 同类型共享页面；页面由包 `ui.pages` 声明，宿主提供组件（overview/dashboard/tags/settings）。
- 用户可增删/改名/排序页面并组合组件；覆盖保存到 workspaces.json，可恢复包默认。
- 类型内实例下拉；仪表盘按设备实例独立。
- 系统页面固定左栏底部入口。

### 与原始计划的差异

- `dashboard` 采用按设备实例保存（而非类型共享布局），与实例下拉语义一致；
- 插件导航页仍属于系统工作区，设备类型专属插件页/`devicePanels` 留后续；
- 包升级不自动合并已保存的工作区页面，需要「恢复包默认页面」。

## 6. 第六轮：报警 + 历史（下一轮）

- **报警**：点级限值（设备包 `alarms` 默认 + 实例覆盖），状态机 active/ack/cleared；
  `ServerMsg::alarms` 推送 + 快照；UI 报警条/报警页/确认操作。
- **历史**：gatewayd 内 SQLite（WAL）+ 批量写（按变化/最小间隔）；表 `tag_history(tag, ts, value, quality)`；
  wire 增加 `getHistory{tags, from, to, maxPoints}`；保留策略（按天/容量）。
- **UI**：ECharts 历史曲线页；报警历史列表。
- **验收**：设限值触发报警（SIM 改值）→ 确认/恢复流转正确；历史查询能画出曲线。

## 7. 第七轮：rhai 脚本驱动 + 打包分发

- **rhai 驱动**：设备包可带 `driver.rhai`（`protocol.driver = "script"`），钩子
  `encode_read/encode_write/decode`；宿主白名单 API（crc16/hex/位运算），预编译校验；
  `Driver` 端口复用现有调度。
- **打包**：Tauri `externalBin` sidecar 打 `gatewayd.exe`；安装包/托盘常驻/开机自启
  （可选 Windows 服务模式）；headless 网关部署文档。
- **验收**：用 rhai 包跑通一台非标协议设备的 poll；安装包在干净机器上开箱可用。

## 8. 新会话开场建议

1. 读本文件 + `README.md`，然后 `cargo test --workspace` 确认基线绿。
2. 需要看 UI 效果时先跑 `cargo run -p hmi-gatewayd -- serve --sim`，用 Node/临时页面验证 WS。
3. 第六轮从「报警引擎」开始：先在 domain/设备包定义点级限值与状态机，再补 wire 快照/推送与
   SQLite 历史表；UI 加报警条/报警页/历史曲线。订阅与质量戳语义沿用现有 TagHub。
4. UI 插件改动先读 `ui/src/plugins/host.ts` 与 `examples/ui-plugins/meter-card`；
   改 `PluginContext`/manifest 契约要同步 `apps/desktop/ui-plugin-sdk/index.d.ts` 与
   `crates/packaging/src/ui_plugin.rs` 的校验。
   UI 改动先跑 `pnpm --dir ui build`（含 vue-tsc），再用浏览器（`?token=`）或 Tauri 窗口验收。
5. 设备工作区改动先读 `ui/src/pages/DeviceWorkspacePage.vue`、`ui/src/utils/devicePages.ts`、
   `crates/gateway/src/workspaces.rs`；新增宿主组件 kind 必须同步 packaging 白名单、
   workspaces 校验、前端组件目录三处。
6. 新增 wire 消息要同时改 `crates/wire`、gatewayd 的 `serve.rs`、`ui/src/api/types.ts`，
   并补 ws_smoke 断言。
7. 保持 crate 边界：`domain/gateway/packaging` 不引入 tauri 依赖。
8. 仪表盘布局改动涉及 `dashboards.json` 契约与保存时校验（`crates/gateway/src/dashboards.rs`）；
   加字段要向后兼容（serde default），非法/重叠布局会被拒绝保存；scope 必须成对出现。
9. 改 Tauri CSP 后必须实测插件加载（`pnpm tauri dev`）：动态 import 跨源需要
   `script-src http://127.0.0.1:59120`，资源请求需要 CORS。
