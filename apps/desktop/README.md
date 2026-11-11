# 桌面端（Tauri 2 + Vue 3）

采集真值全在 `gatewayd`；这里是**薄壳 + WS 客户端**。UI 崩溃/关闭不影响采集。

## 结构

```
src-tauri/        Tauri 2 薄壳：窗口 + gateway_info（读 %APPDATA%/gatewayd/gateway.token）
                 + tauri-plugin-dialog（设备包 zip/目录选择）
ui/               Vue 3 + TS + Vite + Pinia + Naive UI（仅表单）
  src/api/ws.ts     协议客户端单例：握手/重连/请求配对/订阅 refcount/日志
  src/api/bridge.ts 服务端消息 → Pinia 的唯一分发点
  src/stores/       connection / project / tags / dashboards / toast
  src/pages/        诊断 / 仪表盘 / 通道 / 设备 / 设备包 / 实时点表
  src/styles/       theme.css（设计系统）+ pages.css（设备包/仪表盘专用）
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

浏览器模式没有本地路径能力：设备包导入请填 gatewayd 本机绝对路径；Tauri 窗口内支持
把 zip/目录直接拖进导入弹窗，或用「选择 zip / 选择目录」（`tauri-plugin-dialog`）。

## 约定

- `src-tauri` 不写业务逻辑；新增能力先落在 gatewayd + `crates/wire`。
- 新增 wire 消息要同时改 `crates/wire`、`apps/gatewayd/src/serve.rs`、
  `ui/src/api/types.ts`，并补 `ws_smoke` 断言。
- 页面只读 Pinia，不自己解析服务端消息；订阅只覆盖当前可见设备/看板。
- 仪表盘布局真值在 gatewayd（`<data-dir>/dashboards.json`）：组件必须带 `layout` 且互不重叠，
  新增组件字段要向后兼容（serde default / TS 可选）。
- 仪表盘组件契约 `{ id, kind, title, tag, unit, config, layout }` 与未来插件 widget 同构，
  内置渲染器在 `DashboardPage.vue`；未知 `kind` 显示占位而不是崩溃。
- **Volar 坑**：模板静态属性里不要放字面量 `ws://…`（会破坏后续表达式类型解包）。
- **v-model 坑**：`v-model:value` 与 `@update:value` 同时写会互相覆盖（Vue 合并同一监听时的行为）；
  需要“选择后联动”时用 `:value` + `@update:value` 自己赋值（见 DashboardPage 的点位选择）。
- `pnpm --dir ui build`（含 vue-tsc）必须通过再提交。
