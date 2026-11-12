# 桌面端（Tauri 2 + Vue 3）

采集真值全在 `gatewayd`；这里是**薄壳 + WS 客户端**。UI 崩溃/关闭不影响采集。

## 结构

```
src-tauri/        Tauri 2 薄壳：窗口 + gateway_info（读数据目录下的 gateway.token）
                 + supervisor.rs（gatewayd 进程托管：启动拉起/退出回收，只管自己拉起的）
                 + desksettings.rs（desktop.json 数据目录指针）
                 + storage.rs（数据目录查看/迁移命令）
                 + tauri-plugin-dialog（设备包 zip/目录选择）
                 + updates.rs（应用更新：检查/下载/安装/打开发布页）
                 + tauri.release.conf.json（仅 CI 构建叠加：把 gatewayd.exe 打进安装包）
ui/               Vue 3 + TS + Vite + Pinia + Naive UI（仅表单）
  src/api/ws.ts     协议客户端单例：握手/重连/请求配对/订阅 refcount/日志
  src/api/bridge.ts 服务端消息 → Pinia 的唯一分发点
  src/stores/       connection / project / tags / dashboards / workspaces / plugins /
                   updates / toast
  src/pages/        首页 / 诊断 / 全局仪表盘 / 通道 / 设备 / 设备包 / 插件 /
                   插件页面 / 全局点表 / 设备工作区（/t/:typeId/:pageId?）/ 设置
  src/components/device/  设备工作区组件：概览 / 参数设置
  src/utils/devicePages.ts 宿主组件目录、包默认/内置 fallback 页面
  src/plugins/      ESM 插件宿主：动态 import、PluginContext、权限、tag 聚合
  src/styles/       theme.css（设计系统）+ pages.css（设备包/仪表盘/插件/首页/工作区/设置）
ui-plugin-sdk/    插件作者类型定义（SDK v1，index.d.ts；无构建步骤）
```

## 开发

```bash
# 终端 1（仓库根目录）：采集服务
cargo run -p hmi-gatewayd -- serve --sim

# 终端 2
cd apps/desktop
pnpm install
pnpm tauri dev          # 自动拉起 Vite（1420）+ Tauri 窗口
```

浏览器调试（无 Tauri）：

```bash
cd apps/desktop
pnpm --dir ui dev
# 打开 http://localhost:1420/?token=<%APPDATA%/gatewayd/gateway.token 内容>
```

浏览器模式没有本地路径能力：设备包/UI 插件导入请填 gatewayd 本机绝对路径；Tauri 窗口内支持
把 zip/目录直接拖进导入弹窗，或用「选择 zip / 选择目录」（`tauri-plugin-dialog`）。

UI 插件入口由 gatewayd 的 HTTP 静态服务提供（`/plugins/<id>/<entry>`），所以先启动
`gatewayd serve`，再启动 UI；示例插件：`examples/ui-plugins/meter-card`。

## 约定

- `src-tauri` 不写业务逻辑；新增能力先落在 gatewayd + `crates/wire`。
  （supervisor 只做进程托管：探测 59120 → 附着或拉起 `gatewayd serve`（无窗口），
  `RunEvent::ExitRequested` 时只回收自己拉起的子进程；二进制查找顺序 =
  应用目录 gatewayd.exe → `target/release`（开发模式）。）
- 新增 wire 消息要同时改 `crates/wire`、`apps/gatewayd/src/serve.rs`、
  `ui/src/api/types.ts`，并补 `ws_smoke` 断言。
- 设备包/插件删除共用 `valid_id` 白名单（gateway service）：字母数字 + `. - _` 且
  至少一个字母数字 —— 挡住 `.` / `..` 路径穿越；包删除会清 `<packages>/<id>/` 全部
  版本，有设备在用时先拒绝。
- 页面只读 Pinia，不自己解析服务端消息；订阅只覆盖当前可见设备/看板。
- **设备工作区**：顶部类型标签 = 已安装设备包（一包一类型）；`/t/:typeId/:pageId?` 页面按类型共享，
  `?device=` 选实例。页面由包 `ui.pages` 声明（`overview/dashboard/tags/settings`），
  用户覆盖保存到 `<data-dir>/workspaces.json`；`utils/devicePages.ts` 是组件目录与 fallback。
  左侧栏在设备模式显示类型页面、系统模式显示全局页面，底部固定「系统」入口。
- 新增宿主组件 kind 必须同步 `crates/packaging/src/validate.rs` 白名单、
  `crates/gateway/src/workspaces.rs` 校验、`ui/src/utils/devicePages.ts` 与
  `DeviceWorkspacePage.vue` 渲染分支。
- 仪表盘布局真值在 gatewayd（`<data-dir>/dashboards.json`）：组件必须带 `layout` 且互不重叠，
  新增组件字段要向后兼容（serde default / TS 可选）。设备工作区看板带
  `scopeKind="device"` + `scopeId=<device id>`；全局页只看无 scope 的看板。
- 仪表盘组件契约 `{ id, kind, title, tag, unit, config, layout }` 与插件 widget 同构，
  内置渲染器在 `DashboardPage.vue`；插件组件 kind 为 `plugin:<插件id>:<组件id>`，
  由 `PluginWidget.vue` 挂载 `plugins/host.ts` 动态加载的 ESM；未知 `kind` 显示占位而不是崩溃。
- 插件不直接开 WebSocket：`ctx.tags.subscribe/read/write` 走宿主同一套 refcount 订阅与鉴权；
  manifest 未声明的权限会被 runtime 拒绝。入口契约 `export function mount(el, ctx, props)`。
- 改插件 manifest/ctx 契约要同步 `crates/packaging/src/ui_plugin.rs` 校验、
  `ui/src/plugins/host.ts`、`ui/src/api/types.ts`、`ui-plugin-sdk/index.d.ts`。
- Tauri CSP 放行 `127.0.0.1:59120` 的脚本/连接/图片；改 CSP 后用 `pnpm tauri dev` 实测插件加载。
- **Volar 坑**：模板静态属性里不要放字面量 `ws://…`（会破坏后续表达式类型解包）。
- **v-model 坑**：`v-model:value` 与 `@update:value` 同时写会互相覆盖（Vue 合并同一监听时的行为）；
  需要“选择后联动”时用 `:value` + `@update:value` 自己赋值（见 DashboardPage 的点位选择）。
- **数据目录**：默认 `%APPDATA%\gatewayd`，指针存 `%APPDATA%\planM\desktop.json`
  （`desksettings::resolve_data_dir`：指针 > 默认）。迁移（设置页 → 数据目录）只允许在
  gatewayd 由本应用托管时执行：停服务 → 复制（跳过 `logs/`）→ 写指针 → 新目录拉起；
  原目录保留；目标必须为空目录且与当前目录不嵌套。`gateway_info` 与 supervisor
  拉起共用同一解析，二者永不错位。
- **设置页**（/settings）：连接（WS 地址/token 编辑，保存走 `gateway.start` 并重连）与
  应用更新两块。更新状态机在 `stores/updates.ts`（idle/checking/downloading/ready/installing），
  命令契约在 `src-tauri/src/updates.rs`（snake_case 字段）：`get_app_version` /
  `check_for_updates`（读 `releases/latest/download/latest.json` 静态清单，不走限流的
  api.github.com；semver 比较，预发布跳过）/ `download_update{onProgress: Channel}`
  （流式 + sha256 校验）/ `install_update`（复验 → 分离拉起安装程序 → 退出）/
  `open_update_release`。清单与下载 URL 的信任校验在 `trusted_download`（仅本仓库
  Release 直链，拒绝端口/查询串/用户信息）。浏览器调试模式更新区不可用
  （`isTauriRuntime()` 判断）。清单格式见 `scripts/release-artifacts.cjs`：
  `{version, notes, published_at, installers: {"windows-x64": {name, url, size, sha256}}}`。
- **发版构建**：`tauri.release.conf.json` 是**只给 CI 用的叠加配置**
  （`pnpm tauri build -- --bundles nsis --config src-tauri/tauri.release.conf.json`），
  把 `target/release/gatewayd.exe` 作为资源打进安装目录；本地 `pnpm tauri dev` 不引用它
  —— resources 缺失会让 dev 报错，所以绝不能并进主 tauri.conf.json。
  完整发版流程见仓库根 README「发版与更新」。
- `pnpm --dir ui build`（含 vue-tsc）必须通过再提交。
