// plan_M UI 插件 SDK v1（类型定义，仅给插件作者参考；宿主实现在 ui/src/plugins/host.ts）。
//
// 插件入口是一个 ESM 模块，必须导出：
//   export function mount(el, ctx, props): void | (() => void)
// 返回的函数会在组件/页面卸载时调用（解绑事件、清理定时器）。

export type Quality = 'good' | 'bad' | 'uncertain' | 'comm_lost'

export type TagValue =
  | { f64: number }
  | { i64: number }
  | { bool: boolean }
  | { str: string }

export interface TagChange {
  tag: string
  value: TagValue
  quality: Quality
  tsMs: number
}

export interface PluginTagApi {
  /** 订阅 tag；宿主按 tag 聚合去重，并在重连后自动重放订阅。返回退订函数。 */
  subscribe(tags: string[], cb: (changes: TagChange[]) => void): () => void
  /** 当前缓存值（可能为 undefined，表示还没有收到过）。 */
  read(tag: string): TagChange | undefined
  /** 写点；需要 manifest 声明 `tags:write`。拒绝时 Promise 抛错。 */
  write(tag: string, value: TagValue): Promise<void>
}

export interface PluginContext {
  plugin: {
    id: string
    name: string
    version: string
    permissions: string[]
  }
  tags: PluginTagApi
  /** 需要 manifest 声明 `notify`；未声明时调用会被忽略并打印警告。 */
  notify(message: string, kind?: 'info' | 'success' | 'error'): void
  theme: { get(): 'dark' | 'light' }
  log(...args: unknown[]): void
}

export interface PluginWidgetProps {
  kind: 'widget'
  widget: {
    id: string
    title: string
    tag: string
    unit: string
    /** 仪表盘里保存的组件私有 JSON（由插件解释）。 */
    config: Record<string, unknown>
  }
  /** 布局编辑中：建议禁用交互。 */
  editing: boolean
}

export interface PluginPageProps {
  kind: 'page'
  pageId: string
  /** 插件名（页面标题建议用 manifest 里的 page title）。 */
  title: string
}

export type PluginMountProps = PluginWidgetProps | PluginPageProps

export interface PluginModule {
  mount(el: HTMLElement, ctx: PluginContext, props: PluginMountProps): void | (() => void)
}

/** manifest.json 结构（宿主会校验 schemaVersion/sdkVersion/entry/权限/贡献点 id）。 */
export interface PluginManifest {
  schemaVersion: 1
  id: string
  name: string
  version: string
  sdkVersion: 1
  description?: string
  author?: string
  /** 相对插件目录的 ESM 入口，默认 index.js。 */
  entry?: string
  permissions?: Array<'tags:subscribe' | 'tags:write' | 'notify' | 'theme'>
  widgets?: Array<{
    id: string
    name: string
    description?: string
    /** 默认占格（仪表盘 12 列网格）。 */
    defaultW?: number
    defaultH?: number
  }>
  pages?: Array<{ id: string; title: string; icon?: string }>
  /** v1 仅展示，不渲染。 */
  devicePanels?: Array<{ id: string; title: string }>
}
