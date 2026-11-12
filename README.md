# 上位机平台（plan_M）

可扩展、高自定义的工业上位机。核心思路：**声明式设备包 + 双进程架构 + 订阅制实时数据**。

已交付「采集核心 + gatewayd」「Tauri 2 + Vue 3 桌面 UI」「设备包导入 + 仪表盘」
「ESM UI 插件」与「设备工作区」（一个标签 = 一台设备）。
采集进程可独立常驻运行，桌面壳只是它的 WebSocket 客户端；UI 崩溃/关闭不影响采集。

## 架构

```
┌─ UI 进程（Tauri 2 + Vue 3，随时可崩可关）──────────────┐
│ 顶部：设备标签页（一个标签 = 一台设备，"+" 选择设备）     │
│ 左侧：当前设备的页面（设备包声明 + 用户自定义）           │
│       底部固定「切换系统菜单」入口（设备 / 设备包 / 插件 …）
│ 页面组件：概览 / 仪表盘 / 实时点表 / 参数设置 + 插件组件  │
└──────────────┬──────────────────────────────────────┘
               │ WebSocket @127.0.0.1:59120（JSON 消息，token 鉴权）
               │ HTTP /plugins/<id>/<entry>（插件 ESM 静态资源）
┌──────────────┴──────────────┐
│ gatewayd 采集进程（常驻）     │
│ ├─ 通道监督器 × N（串口）     │
│ ├─ 轮询编译器（读块合并）     │
│ ├─ TagHub 实时库（订阅分发）  │
│ ├─ 设备包注册表（声明式）     │
│ ├─ 工作区页面存储             │
│ ├─ UI 插件注册表 + 静态服务   │
│ └─ axum：/health + /ws + /plugins │
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
├─ packaging/         设备包与 UI 插件：manifest/点表/页面声明解析、语义校验、安装、注册表
├─ gateway/           运行时：轮询编译器、通道监督器、TagHub、服务编排、
│                     仪表盘布局（dashboards.json）、设备类型工作区（workspaces.json）
└─ wire/              gatewayd ↔ UI 的 WS 消息契约（serde，两端共用）
apps/
├─ gatewayd/          CLI：serve / poll / scan / pkg；HTTP 静态服务 /plugins/<id>/<entry>
└─ desktop/
   ├─ src-tauri/      Tauri 2 薄壳（gateway_info / 应用更新命令；不写业务逻辑）
   ├─ ui/             Vue 3 + TS + Vite + Pinia（WS 客户端 / 业务页 / 设置页 / 插件宿主）
   └─ ui-plugin-sdk/  插件作者用的 TypeScript 类型（SDK v1，无构建步骤）
scripts/              发版辅助：版本同步 / 一致性校验 / 安装包 staging / Release 校验上传
.github/workflows/    check.yml（构建）+ release.yml（tag → draft Release）
examples/packages/
└─ pm3000/            示例设备包（三相电表，覆盖 u16/u32/i32/cdab/BCD/位展开/可写点/双轮询组）
examples/ui-plugins/
└─ meter-card/        示例 UI 插件（零依赖 ESM：仪表盘组件 + 导航页）
```

## 快速开始

```bash
cargo build --workspace
cargo test --workspace          # 126 个测试：编解码/帧/虚拟总线端到端/包校验/插件安装/工作区/仪表盘/WS 协议/更新校验

# 1) 校验示例设备包
gatewayd pkg validate examples/packages/pm3000

# 2) 无硬件轮询（虚拟从站 + 预置演示数据）
gatewayd poll --package examples/packages/pm3000 --com SIM --once

# 3) 启动服务（--sim = 演示模式：自动装示例包 + SIM 总线 + 两台演示设备）
gatewayd serve --sim
#   → http://127.0.0.1:59120/health
#   → ws://127.0.0.1:59120/ws           （token 见启动输出 / <data-dir>/gateway.token）
#   → http://127.0.0.1:59120/plugins    （已安装 UI 插件清单；资源为 /plugins/<id>/<entry>）
#   → 数据目录：%APPDATA%/gatewayd（--data-dir 可覆盖）
#      config.json / gateway.token / packages/ / ui-plugins/ /
#      workspaces.json / dashboards.json / logs/

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

## 桌面 UI（第二～五轮）

Vue 3 + TypeScript + Vite + Pinia；Naive UI 只做表单控件，外壳/表格/弹窗为自绘，便于后续换肤与插件化。

- **设备工作区**：顶部标签按**设备实例**生成（一个标签 = 一台设备），点「+」弹出设备选择器
  （按总线分组、可搜索）打开/切换标签，标签可关闭并持久化到本地；左侧是该设备所属型号的
  页面导航，底部固定「系统」入口。同型号设备共享页面，页面由设备包 `ui.pages` 声明，
  用户可增删/改名/排序页面并组合宿主组件，保存到 `<data-dir>/workspaces.json`。
  旧类型链接 `/t/<包id>` 自动落到该型号第一台设备，`?device=` 深链直达对应设备。
- **首页**：Hello 欢迎页（时间问候、连接状态、时钟）、关键统计（在线设备/通道/点位/设备包/插件）、
  快速入口与设备概览；根路径 `/` 自动进入首页。
- **诊断**：连接状态、WS 地址/token、`/health` 探测与延迟、运行统计、完整通信日志（收发帧摘要）；
  第二个标签「数据浏览」是跨设备的实时点表（值/单位/质量/更新时间、状态字位展开、`access=rw` 写值）。
- **仪表盘**（宿主组件，仅出现在设备工作区）：12 列网格画布；数值卡 / 开关 / 仪表三种内置组件，
  绑定任意设备 tag；编辑模式拖拽移动、右下角缩放（不允许重叠），改动防抖自动保存到 gatewayd
  （`<data-dir>/dashboards.json`）；开关只对 `access=rw` 的寄存器/线圈点可操作，值来自订阅回读而非本地猜测。
  设备包页面通过 `kind: "dashboard"` 引用它，按 `scopeKind/scopeId` 只展示当前设备的看板；
  跨设备的全局组态尚未提供（预留 `scope=null` 的看板数据）。
- **设备**：按总线（串口通道）分组，组头即总线状态/帧格式/扫描/编辑入口；组内从站卡片显示在线状态、
  设备包版本与点位数，支持添加/删除设备、从扫描结果或设备包页带着型号/从站预填添加；页底为「本机串口」
  枚举（含硬件 ID），可直接用某个口建通道。
- **设备包**：已安装包列表、点表预览（地址/类型/单位/换算/读写/位展开）、在用设备数；
  桌面版支持 **zip/目录拖放** 与文件对话框，浏览器调试模式用 gatewayd 本机路径；导入失败逐字段内联展示，
  成功后自动选中新包并可直接添加设备。
- **插件**：已安装 ESM UI 插件列表（版本/权限/贡献点/加载状态）、zip/目录导入与移除；
  插件贡献的导航页自动出现在侧栏，插件组件自动进入仪表盘「组件类型」下拉框。
- **实时点表**（宿主组件，设备工作区）：按设备查看点位；只订阅当前设备的 tag（bit 子 tag 一并订阅），
  值/单位/质量/更新时间，状态字位展开为芯片，`access=rw` 的点可写值（工程量，gatewayd 负责反算 scale/offset）。
- **设置**：连接（WS 地址/token 编辑，从诊断页迁移）+ 应用与更新（当前版本、检查更新、
  下载进度、安装并重启、打开发布页）；更新命令在 `src-tauri/src/updates.rs`。

基础设施：`ui/src/api/ws.ts` 单例客户端负责 hello/token 握手、指数退避重连、请求响应配对、
**订阅引用计数去重**、重连后自动重放订阅；Pinia 分为 `connection/project/tags/dashboards/workspaces/plugins/toast`。
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
    { "id": "setpoint", "register": "40310", "type": "u16", "access": "rw",
      "min": 0, "max": 50000, "step": 100 },
    { "id": "mode", "register": "40311", "type": "u16", "access": "rw",
      "min": 0, "max": 2, "step": 1,
      "enum": [ { "value": 0, "label": "自动" }, { "value": 1, "label": "手动" } ] }
  ]
}
```

要点：

- **地址用工程记法**（与设备手册口径一致）：`40101` 保持寄存器、`30001` 输入寄存器、
  `00010` 线圈、`10010` 离散量；也支持显式 `HR:100` / `IR:5` / `C:3` / `DR:7`。
- **字节序显式声明**：`abcd`（标准）/ `cdab`（国产电表最常见字交换）/
  `badc`（字内字节交换）/ `dcba`（全反转）。多字值不写会 warning —— 485 头号坑。
- **位展开**：状态字加 `bits` 后自动生成 `<点>.b0` 子 tag（只读）。
- **写值元数据**：`min` / `max` / `step` / `enum` 按**工程量**（scale/offset 换算后）口径，
  供组件生成输入边界、步进与枚举选项；`enum` 仅整数类型，位区域点不支持。
  这些字段会随 `snapshot` 的 `PointDto` 下发，组件写值前由宿主校验范围。
- **轮询组**：按频率分组，编译器把组内相邻寄存器**合并成读块**（间隔 ≤4 合并，≤125 字/块），
  30 点的设备通常只需 3~4 个事务。
- 实例覆盖：设备实例可用 `overrides` 单点改 `scale`/`offset`/`pollGroup`（同型号多台不复制点表）。
- 编辑器支持：`crates/packaging/schema/*.schema.json` 可直接给 VS Code 做补全校验。

## 设备工作区（页面声明）

设备包可在 manifest 里声明该类型的默认页面；**程序只提供组件，页面由包组合**：

```json
"ui": {
  "pages": [
    { "id": "overview", "title": "设备概览", "icon": "pulse",
      "components": [{ "kind": "overview" }] },
    { "id": "monitor", "title": "运行监测", "icon": "activity",
      "components": [
        { "kind": "core:value", "title": "AB 线电压",
          "rect": { "x": 0, "y": 0, "w": 3, "h": 2 },
          "bind": { "value": { "point": "uab" } },
          "config": { "decimals": 1 } },
        { "kind": "core:value", "title": "运行状态",
          "rect": { "x": 3, "y": 0, "w": 3, "h": 2 },
          "bind": { "value": { "point": "status", "bit": 0 } } },
        { "kind": "core:gauge", "title": "有功功率",
          "rect": { "x": 6, "y": 0, "w": 3, "h": 2 },
          "bind": { "value": { "point": "p" } },
          "config": { "min": 0, "max": 50000, "decimals": 0 } },
        { "kind": "core:input", "title": "功率上限设定",
          "rect": { "x": 9, "y": 0, "w": 3, "h": 2 },
          "bind": { "value": { "point": "setpoint" } } }
      ] },
    { "id": "my-page", "title": "我的页面", "icon": "grid",
      "components": [{ "kind": "overview", "title": "状态" },
                     { "kind": "tags", "title": "实时值" }] }
  ]
}
```

宿主组件白名单（v1）：

- `overview`：设备状态/质量统计/通道与包信息；
- `dashboard`：当前设备的仪表盘画布（每个实例独立布局）；
- `tags`：当前设备的实时点表与写值；
- `settings`：设备实例名称/通道/从站/overrides；
- `core:value`（别名 `value`）：绑定一个点/位，显示工程量、质量与单位；
- `core:gauge`（别名 `gauge`）：绑定一个数值点，按 `min/max` 显示弧线刻度；
- `core:switch`（别名 `switch`）：绑定可写点，点击写入 0/1（线圈点写布尔）；
- `core:input`（别名 `input`）：按 `min/max/step/enum` 生成数值输入或枚举下拉；
- `core:bits`（别名 `bits`）：绑定一个字点，按 `bits` 定义逐位展示状态与质量。

规则：

- 包未声明 `ui` 时使用宿主内置四页（概览/仪表盘/实时点表/参数设置），旧包无需修改。
- **绑定是相对点**：`bind` 是「槽位 → `{point, bit}`」；宿主在渲染时解析成
  `<当前设备实例id>.<point>[.b<bit>]`，同一页面可被同型号的多台设备复用。
  `point` 必须存在于本包点表，`bit` 必须在该点 `bits` 里声明过（安装时校验）。
- **绝对 tag 只属于用户页面**：设备包清单里出现 `{ "tag": "pm1.uab" }` 会被安装拒绝；
  用户在工作区编辑页面时可用绝对 tag 做跨设备组态。
- **网格布局**：组件可带 `rect: {x,y,w,h}`（12 列、0 基、行高 72px、间距 12px）；
  整页必须全部声明或全部省略。越界/重叠在安装与保存时拒绝；窄屏自动降为单列。
  不带 `rect` 的页面保持流式排列（连续小组件自动合并为一行网格）。
- `config` 由组件自行解释、宿主原样保存：`core:value` 支持 `decimals`/`precision`/`unit`，
  `core:gauge` 额外支持 `min`/`max`，`core:switch` 支持 `onLabel`/`offLabel`，
  `core:input` 支持覆盖点表的 `min`/`max`/`step`/`decimals`。
- **写值由宿主把关**：`access` 必须为 `rw`；数值按点表 `min/max` 校验；
  位绑定采用“读父点整字 → 改位 → 写回”，点带 `scale/offset` 时拒绝位写（请直接写父点）。
- 页面按**类型共享**；用户在工作区点「编辑页面」可增删页面、改名、排序，并在页面里组合组件。
- 用户修改保存到 `<data-dir>/workspaces.json`；「恢复包默认页面」会删除该类型覆盖。
- 包升级不会自动覆盖已保存的自定义页面；需要新默认页面时点恢复。
- 顶部设备标签 = 已打开的设备实例（一个标签一台设备）；「+」弹出选择器，设备删除后标签自动清掉。
- 设备实例切换只影响组件数据；仪表盘按实例独立保存（`DashboardDto.scopeKind=device`）。

## UI 插件（ESM）

一个目录（或 zip）= `manifest.json` + 构建好的 ESM 入口（默认 `index.js`）+ 资源：

```json
// manifest.json
{
  "schemaVersion": 1,
  "id": "com.example.meter-card",
  "name": "示例插件 · 仪表卡片",
  "version": "1.0.0",
  "sdkVersion": 1,
  "entry": "index.js",
  "permissions": ["tags:subscribe", "notify"],
  "widgets": [
    { "id": "meter", "name": "仪表卡片", "defaultW": 3, "defaultH": 2 }
  ],
  "pages": [
    { "id": "about", "title": "插件示例", "icon": "plugin" }
  ]
}
```

```js
// index.js —— 零依赖、框架无关；返回的函数在卸载时调用
export function mount(el, ctx, props) {
  if (props.kind === 'page') { /* 渲染导航页 */ return () => {} }
  const off = ctx.tags.subscribe([props.widget.tag], (changes) => { /* 画到 el 里 */ })
  return () => off()
}
```

要点：

- **权限是能力门面**：`tags:subscribe` / `tags:write` / `notify` / `theme`，未声明的 ctx 调用直接拒绝；
  未知权限在导入时被校验拒绝（挡住拼写错误）。v1 同 webview 执行，强沙箱留到后续。
- **插件不自己开 WebSocket**：`ctx.tags.subscribe/read/write` 由宿主聚合、按 tag 引用计数去重、断线重连后重放。
- **仪表盘组件** kind 为 `plugin:<插件id>:<组件id>`；布局只保存 kind/tag/config，
  `config` 是插件私有 JSON（示例支持 `min/max/decimals`），宿主原样透传。
- **导航页** 自动注册到侧栏（路由 `/plugin/<插件id>/<页面id>`）；插件删除后导航项与组件一起失效。
- **资源服务**：gatewayd 的 `GET /plugins/<id>/<entry>`（CORS + `no-cache`）；安装落到
  `<data-dir>/ui-plugins/<id>/`，目录为真相源，重启自动扫描。
- **失败可退化**：入口加载失败或插件被删除时，仪表盘组件显示占位符，布局与其它组件不受影响。
- 示例见 `examples/ui-plugins/meter-card`；类型定义见 `apps/desktop/ui-plugin-sdk/index.d.ts`。

## WS 协议（摘要，完整见 `crates/wire`）

客户端 → 服务端：`hello{token}` / `getSnapshot` / `subscribe{tags}` / `unsubscribe` /
`writeTag{tag,value}` / `scanChannel{channel,range}` / `getSerialPorts` / `getPackages` /
`upsertChannel` / `removeChannel` / `addDevice` / `removeDevice` / `importPackage{path}` /
`removePackage{id}`（有设备在用时拒绝）/
`getDashboards` / `saveDashboard{dashboard}` / `removeDashboard{id}` /
`getUiPlugins` / `importUiPlugin{path}` / `removeUiPlugin{id}` /
`getWorkspaces` / `saveWorkspace{workspace}` / `removeWorkspace{deviceType}`

服务端 → 客户端：`welcome` / `snapshot` / `changes{items}`（10~20ms 合并批量）/
`channelState` / `deviceState` / `scanProgress` / `scanDone` / `serialPorts` / `packages` /
`dashboards` / `uiPlugins` / `workspaces` / `commandOk` / `error`

仪表盘布局（`DashboardDto`）：`{ id, name, cols, scopeKind?, scopeId?, widgets[] }`；widget 为
`{ id, kind, title, tag, unit, config, layout{x,y,w,h} }`。`config` 由组件自行解释
（内置 value=`decimals`、switch=`onLabel/offLabel`、gauge=`min/max`，插件组件由插件解释）。
`scopeKind/scopeId` 缺省 = 系统全局看板；`device` = 某台设备工作区看板。
gatewayd 保存时校验 id/边界/**组件不重叠**/scope，布局属工程数据，UI 可重装。

UI 插件（`UiPluginDto`）：`{ id, name, version, sdkVersion, permissions[], entry, widgets[], pages[], devicePanels[] }`。
`getUiPlugins/importUiPlugin/removeUiPlugin` 统一以 `uiPlugins` 返回完整列表；安装支持目录或 zip。

设备工作区（`WorkspaceDto`）：`{ deviceType, pages[] }`；page 为
`{ id, title, icon, components[{ kind, title, bind?, rect?, config }] }`，组件绑定为
`{ 槽位: { point, bit } | { tag } }`，`rect` 为 `{ x, y, w, h }`（12 列网格）。`deviceType` 是设备包 id；
`getWorkspaces/saveWorkspace/removeWorkspace` 统一以 `workspaces` 返回完整列表。
`PackageDto.ui.pages` 提供包默认页面，`workspaces` 保存用户覆盖。

## 发版与更新

版本号单一真值是根 `Cargo.toml` 的 `[workspace.package] version`（所有 crate
`version.workspace = true`，gatewayd 的 `/health` 与桌面壳版本自动跟随）；另需同步
`apps/desktop/src-tauri/tauri.conf.json` 与两个 `package.json` —— 用脚本一次改全：

```bash
node scripts/set-version.mjs 0.2.0   # 同步 4 处版本号并刷新 Cargo.lock
git commit -am "chore: v0.2.0"
git tag v0.2.0 && git push origin master v0.2.0
```

GitHub Actions（与 pika_client 同一套机制）：

- **check.yml**：push master / PR 时在 windows-latest 上跑 fmt + clippy + 全量测试，
  构建 `gatewayd.exe` 与 NSIS 安装包（`--config src-tauri/tauri.release.conf.json`
  叠加把 gatewayd.exe 打进安装目录），staging 出带 sha256/来源树哈希的工件。
- **release.yml**：推 `v*` tag 后校验版本一致性 → 找到与 tag 源码树一致的成功 CI 构建 →
  复验工件摘要 → 创建 **draft Release** 并附安装包 + `latest.json` 更新清单
  （版本/说明/各平台安装包的规范资产名、百分号编码 URL、sha256）；手动确认后 publish。
  只提升「构建过且验证过」的产物，绝不改已发布的 Release。

安装包内容：`planM_<版本>_x64-setup.exe`（含桌面应用 + 同版本 gatewayd.exe，
装到同一目录）。**打开即用**：桌面壳启动时自动拉起 gatewayd（后台无窗口），
退出时自动回收；已有外部 gatewayd 在跑则直接附着。数据目录在
`%APPDATA%/gatewayd`。

**数据目录**：默认 `%APPDATA%\gatewayd`（Windows 规范位置，升级/卸载不影响数据）。
不喜欢放 C 盘的用户可在 **设置 → 数据目录** 迁移：选择一个空目录，桌面壳会复制全部
数据（不含日志）、写入指针并按新目录重启采集服务，**原目录保留作备份**。目录指针存于
`%APPDATA%\planM\desktop.json`（固定，几十字节；安装向导选盘只影响程序本身）。

**应用内更新**（设置 → 应用与更新）：自研命令而非 tauri-plugin-updater —— 读 Release
附带的静态清单 `releases/latest/download/latest.json`（不走 api.github.com，避开 60 次/小时
限流；CDN 缓存导致发布后最长延迟约 1 小时可见）→ semver 比较（预发布版本跳过）→ 流式下载
`_x64-setup.exe` 并校验清单里的 sha256 → 分离进程拉起安装程序并退出。信任边界：只接受
`https://github.com/mkktop/plan_M/releases/download/` 直链（拒绝端口/查询串/用户信息）+
平台后缀 + 合法摘要；更新逻辑全在 Rust 侧（`src-tauri/src/updates.rs`），不经 webview 网络。
浏览器调试模式更新区不可用（连接设置仍可编辑）。

## 下一轮：报警 + 历史

完整计划、验收标准与交接说明见 **[docs/ROADMAP.md](docs/ROADMAP.md)**。概要：

1. 报警引擎（点级限值 + active/ack/cleared）与报警 UI
2. 历史归档（SQLite WAL + 保留策略）与历史曲线
3. rhai 脚本驱动（私有协议，沙箱内跑）+ 打包分发

## 已知边界（刻意不做）

- 无 TLS（本地回环服务，token 鉴权已够用；跨机部署时再加）
- 安装包未做代码签名：Windows SmartScreen 会提示「更多信息 → 仍要运行」；
  应用内更新依赖 GitHub 资产 sha256 摘要做完整性校验（不做来源签名验证）
- 桌面壳自动拉起/回收 gatewayd，但**只管理自己拉起的实例**：手动运行的
  gatewayd 一律附着不接管；应用异常崩溃时自己拉起的实例会留成孤儿，
  下次启动按已有实例附着（不重复拉起、不丢采集）
- 常驻驻留（托盘/开机自启）与 Windows 服务模式留到后续轮次
- 浏览器调试模式导入设备包需要 gatewayd 本机路径；拖放/文件对话框只在桌面壳内可用
- 仪表盘布局是「最后写入者胜」，多人同时编辑同一看板不做冲突合并
- 设备类型 = 设备包；同一类型共享页面配置，但仪表盘按设备实例独立保存
- 用户保存的工作区页面不会随设备包升级自动合并；需要新默认页面时点「恢复包默认页面」
- 历史/报警未落库（SQLite 已留好位置）
- `scanChannel` 在 WS 会话内同步执行（几十秒级；扫描期间该连接的其他请求会排队）
- UI 插件 v1 与宿主同 webview：manifest 权限是能力门面而非强沙箱；`devicePanels` 贡献点已解析但不渲染
- 插件贡献的导航页目前属于系统工作区；设备类型专属插件页/面板留到后续
- Tauri CSP 已启用（放行 `127.0.0.1:59120` 的脚本/连接/图片；dev 模式另放行 1420）
