# meter-card（示例 UI 插件）

零依赖、纯 ESM、框架无关的插件示例：一个仪表盘组件 + 一个导航页。

## 试一下

```bash
# 1. 启动采集服务
cargo run -p hmi-gatewayd -- serve --sim

# 2. 启动桌面 UI（或浏览器模式 ?token=...）
cd apps/desktop && pnpm tauri dev
```

打开「插件」页 → 导入插件 → 填本目录的绝对路径（或拖入目录/zip）。安装后：

- 侧栏出现「插件示例」导航页；
- 仪表盘 → 添加组件 → 类型里出现「仪表卡片 · 示例插件 · 仪表卡片」；
- 绑定 `pm1.uab`，config 填 `{"min":180,"max":260,"decimals":1}`，即可看到 220.1V 的实时卡片。

## 契约

```js
export function mount(el, ctx, props) {
  // props.kind === 'widget'：props.widget = { id, title, tag, unit, config }
  // props.kind === 'page'  ：props.pageId / props.title
  // 返回的函数在组件/页面卸载时调用
}
```

- `ctx.tags.subscribe(tags, cb)`：宿主按 tag 聚合去重；`ctx.tags.read/write` 需对应权限。
- manifest 权限：`tags:subscribe` / `tags:write` / `notify` / `theme`。
- 组件 kind：`plugin:com.example.meter-card:meter`，由宿主自动生成，布局只保存 kind/tag/config。
- 完整类型见 `apps/desktop/ui-plugin-sdk/index.d.ts`。

## 设备页（v1 兼容 + v2 槽位）

同一份组件也会出现在设备页编辑器的组件下拉里（kind 仍是
`plugin:com.example.meter-card:meter`）。v1 的 `mount(el, ctx, props)` 由宿主自动适配：
`props.widget.tag` 取组件 `bind.value` 解析后的 tag，`ctx.tags` 被限制在该组件声明的绑定内。

新插件可以导出 v2 入口，直接按槽位取写值（ShadowRoot + 主题变量 + 权限门面）：

```js
export function mountWidget(host) {
  const off = host.slots.sub('value', ([s]) => {
    host.el.textContent = `${s.sample.value.f64 ?? s.sample.value.i64} ${s.meta.unit}`
  })
  host.slots.read('value')  // 首帧
  return () => off()
}
```

设备包自带的 ESM 组件（`ui.widgets[]`）使用同一套 `WidgetHost` 接口；
示例见 `examples/packages/pm3000/widgets/trend.js`。
