//! UI 插件宿主：动态 import gatewayd 提供的 ESM 入口，注入受控 `ctx`。
//!
//! 设计要点：
//! - 插件不直接开 WebSocket；`ctx.tags.subscribe` 统一走宿主的引用计数客户端；
//! - 权限是能力门面：manifest 未声明的权限，对应 ctx 方法直接拒绝；
//! - 插件 widget 只负责把值画到自己的 DOM 里，布局/保存仍由仪表盘宿主负责。

import { reactive } from 'vue'
import { gateway } from '@/api/ws'
import { useTagsStore } from '@/stores/tags'
import { useToastStore } from '@/stores/toast'
import type { TagChange, TagValue, UiPluginDto, UiPageDto, UiWidgetDto, WidgetDto } from '@/api/types'

export const PLUGIN_KIND_PREFIX = 'plugin:'

export type PluginNotifyKind = 'info' | 'success' | 'error'

export interface PluginTagApi {
  subscribe(tags: string[], cb: (changes: TagChange[]) => void): () => void
  read(tag: string): TagChange | undefined
  write(tag: string, value: TagValue): Promise<void>
}

export interface PluginContext {
  plugin: { id: string; name: string; version: string; permissions: string[] }
  tags: PluginTagApi
  notify(message: string, kind?: PluginNotifyKind): void
  theme: { get(): 'dark' | 'light' }
  log(...args: unknown[]): void
}

export interface PluginWidgetMount {
  kind: 'widget'
  widget: {
    id: string
    title: string
    tag: string
    unit: string
    config: Record<string, unknown>
  }
  editing: boolean
}

export interface PluginPageMount {
  kind: 'page'
  pageId: string
  title: string
}

export type PluginMountProps = PluginWidgetMount | PluginPageMount

export interface PluginModule {
  mount(el: HTMLElement, ctx: PluginContext, props: PluginMountProps): void | (() => void)
}

export interface PluginStatus {
  id: string
  name: string
  version: string
  description: string
  author: string
  sdkVersion: number
  permissions: string[]
  entry: string
  widgetCount: number
  pageCount: number
  error: string
}

export interface PluginNavPage {
  pluginId: string
  pluginName: string
  pageId: string
  title: string
  icon: string
  path: string
}

interface LoadedPlugin {
  dto: UiPluginDto
  module: PluginModule | null
  error: string
}

/** 响应式宿主状态：`version` 变化时，UI 重新解析组件/导航。 */
export const pluginHost = reactive({
  version: 0,
  statuses: [] as PluginStatus[],
  pages: [] as PluginNavPage[],
})

const loaded = new Map<string, LoadedPlugin>()
const widgetIndex = new Map<string, { plugin: LoadedPlugin; widget: UiWidgetDto }>()
const pageIndex = new Map<string, { plugin: LoadedPlugin; page: UiPageDto }>()
/** 已加载模块的入口 URL（插件页/管理页展示用）。 */
const moduleUrls = new Map<string, string>()

export function pluginKind(pluginId: string, widgetId: string): string {
  return `${PLUGIN_KIND_PREFIX}${pluginId}:${widgetId}`
}

export function parsePluginKind(kind: string): { pluginId: string; widgetId: string } | null {
  if (!kind.startsWith(PLUGIN_KIND_PREFIX)) return null
  const rest = kind.slice(PLUGIN_KIND_PREFIX.length)
  const i = rest.indexOf(':')
  if (i <= 0 || i === rest.length - 1) return null
  return { pluginId: rest.slice(0, i), widgetId: rest.slice(i + 1) }
}

/** 从 WS 地址推导 gatewayd 的 HTTP 源（插件资源走 HTTP，不走 WS）。 */
export function gatewayHttpBase(): string {
  const raw = gateway.state.settings.wsUrl || 'ws://127.0.0.1:59120/ws'
  try {
    const u = new URL(raw)
    u.protocol = u.protocol === 'wss:' ? 'https:' : 'http:'
    u.pathname = ''
    u.search = ''
    u.hash = ''
    return u.origin
  } catch {
    return 'http://127.0.0.1:59120'
  }
}

export function pluginModuleUrl(dto: Pick<UiPluginDto, 'id' | 'entry' | 'version'>): string {
  const entry = dto.entry
    .split(/[\\/]+/)
    .filter(Boolean)
    .map(encodeURIComponent)
    .join('/')
  return `${gatewayHttpBase()}/plugins/${encodeURIComponent(dto.id)}/${entry}?v=${encodeURIComponent(dto.version)}`
}

export function pluginModuleUrlOf(pluginId: string): string {
  return moduleUrls.get(pluginId) ?? ''
}

let syncChain: Promise<void> = Promise.resolve()

/** WS 返回插件列表后调用；串行化，避免并发 sync 互相覆盖。 */
export function syncPlugins(list: UiPluginDto[]): Promise<void> {
  syncChain = syncChain
    .then(() => doSync(list))
    .catch((e) => console.error('[plugin host] 同步插件失败', e))
  return syncChain
}

async function doSync(list: UiPluginDto[]): Promise<void> {
  const incoming = new Map(list.map((p) => [p.id, p]))
  for (const id of [...loaded.keys()]) {
    const next = incoming.get(id)
    const cur = loaded.get(id)
    if (!next || !cur || next.version !== cur.dto.version || cur.error) {
      loaded.delete(id)
      moduleUrls.delete(id)
    }
  }
  for (const dto of list) {
    if (!loaded.has(dto.id)) await loadPlugin(dto)
  }
  rebuildIndexes()
}

async function loadPlugin(dto: UiPluginDto): Promise<void> {
  const url = pluginModuleUrl(dto)
  const entry: LoadedPlugin = { dto, module: null, error: '' }
  loaded.set(dto.id, entry)
  moduleUrls.set(dto.id, url)
  try {
    const mod = (await import(/* @vite-ignore */ url)) as PluginModule
    if (!mod || typeof mod.mount !== 'function') {
      throw new Error('入口必须导出 mount(el, ctx, props)')
    }
    entry.module = mod
    console.info(`[plugin ${dto.id}] 已加载 v${dto.version} ← ${url}`)
  } catch (e) {
    entry.error = e instanceof Error ? e.message : String(e)
    console.error(`[plugin ${dto.id}] 加载失败`, e)
  }
}

function rebuildIndexes(): void {
  widgetIndex.clear()
  pageIndex.clear()
  const statuses: PluginStatus[] = []
  const pages: PluginNavPage[] = []
  const sorted = [...loaded.values()].sort((a, b) => a.dto.id.localeCompare(b.dto.id))
  for (const plugin of sorted) {
    const dto = plugin.dto
    statuses.push({
      id: dto.id,
      name: dto.name,
      version: dto.version,
      description: dto.description,
      author: dto.author,
      sdkVersion: dto.sdkVersion,
      permissions: dto.permissions,
      entry: dto.entry,
      widgetCount: dto.widgets.length,
      pageCount: dto.pages.length,
      error: plugin.error,
    })
    if (!plugin.module) continue
    for (const widget of dto.widgets) {
      widgetIndex.set(pluginKind(dto.id, widget.id), { plugin, widget })
    }
    for (const page of dto.pages) {
      pageIndex.set(`${dto.id}:${page.id}`, { plugin, page })
      pages.push({
        pluginId: dto.id,
        pluginName: dto.name,
        pageId: page.id,
        title: page.title,
        icon: page.icon || 'plugin',
        path: `/plugin/${dto.id}/${page.id}`,
      })
    }
  }
  pluginHost.statuses = statuses
  pluginHost.pages = pages
  pluginHost.version += 1
}

/** 解析插件 widget（模板里调用；读取 version 建立响应式依赖）。 */
export function pluginWidgetInfo(
  kind: string,
): { plugin: UiPluginDto; widget: UiWidgetDto } | null {
  void pluginHost.version
  const hit = widgetIndex.get(kind)
  return hit ? { plugin: hit.plugin.dto, widget: hit.widget } : null
}

/** 可加入组件类型下拉框的插件组件。 */
export function pluginWidgetOptions(): { kind: string; label: string; widget: UiWidgetDto }[] {
  void pluginHost.version
  return [...widgetIndex.entries()].map(([kind, hit]) => ({
    kind,
    label: `${hit.widget.name} · ${hit.plugin.dto.name}`,
    widget: hit.widget,
  }))
}

export function pluginPageInfo(
  pluginId: string,
  pageId: string,
): { plugin: UiPluginDto; page: UiPageDto } | null {
  void pluginHost.version
  const hit = pageIndex.get(`${pluginId}:${pageId}`)
  return hit ? { plugin: hit.plugin.dto, page: hit.page } : null
}

// —— 实时值聚合（所有插件共用一条 gateway.onMessage 监听）——

type TagListener = (changes: TagChange[]) => void

class PluginTagBus {
  private handlers = new Map<string, Set<TagListener>>()
  private installed = false

  private ensureListener(): void {
    if (this.installed) return
    this.installed = true
    gateway.onMessage((msg) => {
      if (msg.type !== 'changes') return
      for (const change of msg.items) {
        const set = this.handlers.get(change.tag)
        if (!set) continue
        for (const cb of set) {
          try {
            cb([change])
          } catch (e) {
            console.error('[plugin] tag 回调异常', e)
          }
        }
      }
    })
  }

  subscribe(tags: string[], cb: TagListener): () => void {
    this.ensureListener()
    const clean = [...new Set(tags.map((t) => t.trim()).filter(Boolean))]
    if (!clean.length) return () => {}
    for (const tag of clean) {
      let set = this.handlers.get(tag)
      if (!set) {
        set = new Set()
        this.handlers.set(tag, set)
      }
      set.add(cb)
    }
    const off = gateway.subscribe(clean)
    const initial = clean
      .map((t) => useTagsStore().get(t))
      .filter((v): v is TagChange => Boolean(v))
    if (initial.length) {
      queueMicrotask(() => {
        try {
          cb(initial)
        } catch (e) {
          console.error('[plugin] tag 首值回调异常', e)
        }
      })
    }
    let active = true
    return () => {
      if (!active) return
      active = false
      for (const tag of clean) {
        const set = this.handlers.get(tag)
        if (!set) continue
        set.delete(cb)
        if (!set.size) this.handlers.delete(tag)
      }
      off()
    }
  }
}

const tagBus = new PluginTagBus()

function requirePermission(plugin: LoadedPlugin, permission: string): void {
  if (!plugin.dto.permissions.includes(permission)) {
    throw new Error(`插件 ${plugin.dto.id} 未声明权限 \`${permission}\`，能力已拒绝`)
  }
}

function createContext(plugin: LoadedPlugin): PluginContext {
  const dto = plugin.dto
  return {
    plugin: {
      id: dto.id,
      name: dto.name,
      version: dto.version,
      permissions: [...dto.permissions],
    },
    tags: {
      subscribe(tags, cb) {
        requirePermission(plugin, 'tags:subscribe')
        return tagBus.subscribe(tags, cb)
      },
      read(tag) {
        return useTagsStore().get(tag)
      },
      async write(tag, value) {
        requirePermission(plugin, 'tags:write')
        await gateway.request({ type: 'writeTag', tag, value }, ['commandOk', 'error'], 8000)
      },
    },
    notify(message, kind = 'info') {
      if (!dto.permissions.includes('notify')) {
        console.warn(`[plugin ${dto.id}] 未声明 notify 权限，通知被忽略`)
        return
      }
      const toast = useToastStore()
      if (kind === 'success') toast.success(message)
      else if (kind === 'error') toast.error(message)
      else toast.info(message)
    },
    theme: {
      get: () => 'dark',
    },
    log(...args) {
      console.info(`[plugin ${dto.id}]`, ...args)
    },
  }
}

/** 挂载插件 widget；返回清理函数（卸载 DOM 由调用方负责）。 */
export function mountPluginWidget(
  kind: string,
  el: HTMLElement,
  widget: WidgetDto,
  editing: boolean,
): () => void {
  const hit = widgetIndex.get(kind)
  if (!hit) {
    const parsed = parsePluginKind(kind)
    throw new Error(parsed ? `插件 ${parsed.pluginId} 未安装或未加载` : `未知组件类型 ${kind}`)
  }
  if (!hit.plugin.module) throw new Error(hit.plugin.error || '插件入口未加载')

  el.replaceChildren()
  const props: PluginWidgetMount = {
    kind: 'widget',
    widget: {
      id: widget.id,
      title: widget.title,
      tag: widget.tag,
      unit: widget.unit,
      config: widget.config ?? {},
    },
    editing,
  }
  const result = hit.plugin.module.mount(el, createContext(hit.plugin), props)
  let active = true
  return () => {
    if (!active) return
    active = false
    try {
      if (typeof result === 'function') result()
    } catch (e) {
      console.error('[plugin] 卸载组件异常', e)
    } finally {
      el.replaceChildren()
    }
  }
}

/** 挂载插件导航页；返回清理函数。 */
export function mountPluginPage(pluginId: string, pageId: string, el: HTMLElement): () => void {
  const hit = pageIndex.get(`${pluginId}:${pageId}`)
  if (!hit) throw new Error(`插件 ${pluginId} 的页面 ${pageId} 未安装或未加载`)
  if (!hit.plugin.module) throw new Error(hit.plugin.error || '插件入口未加载')

  el.replaceChildren()
  const props: PluginPageMount = { kind: 'page', pageId, title: hit.plugin.dto.name }
  const result = hit.plugin.module.mount(el, createContext(hit.plugin), props)
  let active = true
  return () => {
    if (!active) return
    active = false
    try {
      if (typeof result === 'function') result()
    } catch (e) {
      console.error('[plugin] 卸载页面异常', e)
    } finally {
      el.replaceChildren()
    }
  }
}
