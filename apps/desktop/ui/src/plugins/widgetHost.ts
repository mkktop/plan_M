//! 外部小组件宿主（P3）：插件 / 设备包 ESM 组件进设备页的统一运行时。
//!
//! - v2 组件：`mountWidget(host)` 或 `mount(host)`，宿主注入 ShadowRoot、槽位、
//!   主题与权限门面；组件不接触 tag 寻址。
//! - v1 插件兼容：`mount(el, ctx, props)` 仍在 light DOM 挂载（插件样式是全局的，
//!   Shadow 内反而失效）；`ctx.tags` 被限制在该组件已声明的绑定范围内。
//!
//! 槽位订阅按「可见性 + tag 引用计数」管理：不可见时退订，进入视口（提前 200px）再补。

import { gateway } from '@/api/ws'
import { useProjectStore } from '@/stores/project'
import { useTagsStore } from '@/stores/tags'
import { useToastStore } from '@/stores/toast'
import { bindingDecls, resolveBoundTag } from '@/utils/devicePages'
import { boolOf, primitiveOf } from '@/utils/format'
import type {
  BindingDecl,
  DevicePageComponentDto,
  PackageDto,
  PointDto,
  TagChange,
  TagValue,
} from '@/api/types'
import type { ExternalWidgetDescriptor } from './deviceWidgets'

// —— 主题 token（与宿主 UI 的颜色保持一致）——

const TOKENS: Record<string, string> = {
  'color.bg': '#0a0d12',
  'color.panel': 'rgba(255, 255, 255, 0.035)',
  'color.text': '#e8eef7',
  'color.textMuted': '#8494a8',
  'color.accent': '#4db5ff',
  'color.good': '#35d399',
  'color.bad': '#ff6b6b',
  'color.warn': '#f5b942',
  'color.border': '#26313f',
  'font.mono': 'ui-monospace, "Cascadia Mono", Consolas, monospace',
}

const HOST_CSS = `
:host {
  all: initial;
  display: block;
  height: 100%;
  box-sizing: border-box;
  color: var(--pm-color-text);
  font: 13px/1.5 Inter, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
}
.pm-widget-root { height: 100%; min-height: 0; box-sizing: border-box; }
`

function hostCssVars(): string {
  return `
:host {
  --pm-color-bg: ${TOKENS['color.bg']};
  --pm-color-panel: ${TOKENS['color.panel']};
  --pm-color-text: ${TOKENS['color.text']};
  --pm-color-text-muted: ${TOKENS['color.textMuted']};
  --pm-color-accent: ${TOKENS['color.accent']};
  --pm-color-good: ${TOKENS['color.good']};
  --pm-color-bad: ${TOKENS['color.bad']};
  --pm-color-warn: ${TOKENS['color.warn']};
  --pm-color-border: ${TOKENS['color.border']};
  --pm-font: Inter, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif;
  --pm-font-mono: ${TOKENS['font.mono']};
}
`
}

// —— PointMeta / 槽位样本 ——

export interface PointMeta {
  key: string
  tag: string
  name: string
  unit: string
  kind: 'number' | 'bool' | 'enum'
  access: 'r' | 'rw'
  dataType: string
  scale?: number
  offset?: number
  min?: number
  max?: number
  step?: number
  enum?: { value: number; label: string }[]
  bits?: { bit: number; name: string; on?: string; off?: string }[]
}

export interface SlotSample {
  meta: PointMeta
  sample: TagChange
}

export interface WidgetHost {
  el: HTMLElement
  mode: 'view' | 'edit'
  config: Readonly<Record<string, unknown>>
  slots: {
    sub(slot: string, cb: (samples: SlotSample[]) => void): () => void
    read(slot: string): SlotSample[] | undefined
    meta(slot: string): PointMeta | undefined
    write(slot: string, value: TagValue): Promise<void>
  }
  theme: { token(name: string): string; mode(): 'dark' | 'light' }
  notify(message: string, kind?: 'info' | 'success' | 'error'): void
  log(...args: unknown[]): void
}

interface MetaHit {
  meta: PointMeta
  point: PointDto
}

/** tag → PointMeta（设备点；`$` 系统 tag 暂不支持槽位）。 */
function resolveMeta(tag: string, fallbackPkg?: PackageDto | null): MetaHit | null {
  if (!tag || tag.startsWith('$')) return null
  const [deviceId, pointId, bitId] = tag.split('.')
  if (!deviceId || !pointId) return null
  const project = useProjectStore()
  const device = project.devices.find((d) => d.id === deviceId)
  const pkg = (device ? project.packageById.get(device.package) : null) ?? fallbackPkg ?? null
  const point = pkg?.points.find((p) => p.id === pointId)
  if (!point) return null
  const bit = bitId?.startsWith('b') ? point.bits.find((b) => `b${b.bit}` === bitId) : undefined
  if (bitId && !bit) return null

  const enumList = (point.enum ?? []).map((e) => ({ value: e.value, label: e.label }))
  const kind: PointMeta['kind'] =
    bit || point.area === 'coil' || point.area === 'discrete'
      ? 'bool'
      : enumList.length
        ? 'enum'
        : 'number'
  const meta: PointMeta = {
    key: `${point.id}${bit ? `.b${bit.bit}` : ''}`,
    tag,
    name: bit ? `${point.name} · ${bit.name}` : point.name,
    unit: point.unit ?? '',
    kind,
    access: point.access,
    dataType: point.area === 'coil' || point.area === 'discrete' ? 'bool' : point.type,
    scale: point.scale ?? undefined,
    offset: point.offset ?? undefined,
    min: point.min ?? undefined,
    max: point.max ?? undefined,
    step: point.step ?? undefined,
    enum: enumList.length ? enumList : undefined,
    bits: point.bits.length
      ? point.bits.map((b) => ({
          bit: b.bit,
          name: b.name,
          on: b.on ?? undefined,
          off: b.off ?? undefined,
        }))
      : undefined,
  }
  return { meta, point }
}

// —— 实时值总线（单条 onMessage 监听，按 tag 分发）——

type TagListener = (changes: TagChange[]) => void

class TagBus {
  private handlers = new Map<string, Set<TagListener>>()
  private installed = false

  private ensure(): void {
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
            console.error('[widget] tag 回调异常', e)
          }
        }
      }
    })
  }

  subscribe(tags: string[], cb: TagListener): () => void {
    this.ensure()
    const clean = [...new Set(tags.filter(Boolean))]
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

const tagBus = new TagBus()

function sampleOf(tag: string, pkg?: PackageDto | null): SlotSample | undefined {
  const change = useTagsStore().get(tag)
  if (!change) return undefined
  const hit = resolveMeta(tag, pkg)
  if (!hit) return undefined
  return { meta: hit.meta, sample: change }
}

class SlotChannel {
  private handlers = new Set<(samples: SlotSample[]) => void>()
  private off: (() => void) | null = null
  tags: string[] = []

  constructor(private readonly pkg?: PackageDto | null) {}

  setTags(tags: string[]): void {
    if (tags.join('|') === this.tags.join('|')) return
    this.tags = tags
    if (this.handlers.size) this.attach()
  }

  attach(): void {
    this.detach()
    if (!this.tags.length) return
    this.off = tagBus.subscribe(this.tags, (changes) => {
      const samples: SlotSample[] = []
      for (const change of changes) {
        const hit = resolveMeta(change.tag, this.pkg)
        if (hit) samples.push({ meta: hit.meta, sample: change })
      }
      if (samples.length) this.emit(samples)
    })
  }

  detach(): void {
    this.off?.()
    this.off = null
  }

  emit(samples: SlotSample[]): void {
    for (const cb of this.handlers) {
      try {
        cb(samples)
      } catch (e) {
        console.error('[widget] 槽位回调异常', e)
      }
    }
  }

  readAll(): SlotSample[] {
    const out: SlotSample[] = []
    for (const tag of this.tags) {
      const s = sampleOf(tag, this.pkg)
      if (s) out.push(s)
    }
    return out
  }

  get listeners(): number {
    return this.handlers.size
  }

  add(cb: (samples: SlotSample[]) => void): boolean {
    this.handlers.add(cb)
    return this.handlers.size === 1
  }

  remove(cb: (samples: SlotSample[]) => void): boolean {
    this.handlers.delete(cb)
    return this.handlers.size === 0
  }
}

// —— 模块加载缓存（URL 带版本串，升级自动失效）——

const moduleCache = new Map<string, Promise<unknown>>()

function importModule(url: string): Promise<Record<string, unknown>> {
  let hit = moduleCache.get(url)
  if (!hit) {
    hit = import(/* @vite-ignore */ url)
    moduleCache.set(url, hit)
  }
  return hit.then((m) => m as Record<string, unknown>)
}

// —— 挂载 ——

export interface MountExternalOptions {
  el: HTMLElement
  descriptor: ExternalWidgetDescriptor
  deviceId: string
  component: DevicePageComponentDto
  pkg?: PackageDto | null
  mode?: 'view' | 'edit'
}

export interface WidgetInstance {
  /** 返回 true = 已原地 update；false = 需要调用方重挂 */
  update(): boolean
  dispose(): void
}

interface SlotTags {
  [slot: string]: string[]
}

function slotTagsOf(
  deviceId: string,
  bind: DevicePageComponentDto['bind'],
): SlotTags {
  const out: SlotTags = {}
  for (const [slot, spec] of Object.entries(bind ?? {})) {
    out[slot] = bindingDecls(spec)
      .map((d) => resolveBoundTag(deviceId, d))
      .filter(Boolean)
  }
  return out
}

function firstDecl(bind: DevicePageComponentDto['bind'], slot: string): BindingDecl | undefined {
  return bindingDecls(bind?.[slot])[0]
}

function permissionSet(descriptor: ExternalWidgetDescriptor): Set<string> {
  return new Set(descriptor.permissions ?? [])
}

function canRead(perms: Set<string>): boolean {
  return perms.has('tags:read') || perms.has('tags:subscribe')
}

function numericTagValue(point: PointDto, v: number): TagValue {
  if (point.area === 'coil' || point.area === 'discrete') return { bool: v !== 0 }
  if (point.type === 'f32' || point.type === 'f64') return { f64: v }
  return { i64: Math.round(v) }
}

async function writeTag(tag: string, value: TagValue): Promise<void> {
  await gateway.request({ type: 'writeTag', tag, value }, ['commandOk', 'error'], 8000)
}

export async function mountExternalWidget(opts: MountExternalOptions): Promise<WidgetInstance> {
  const { descriptor, component, deviceId, mode = 'view' } = opts
  const pkg = opts.pkg ?? null
  const mod = await importModule(descriptor.moduleUrl)

  const mountWidget = mod.mountWidget
  const mountAny = mod.mount
  const legacy =
    typeof mountWidget !== 'function' && typeof mountAny === 'function' && mountAny.length >= 2

  if (legacy) {
    return mountLegacy({
      el: opts.el,
      descriptor,
      component,
      deviceId,
      pkg,
      mode,
      module: mod,
    })
  }

  const fn = (typeof mountWidget === 'function' ? mountWidget : mountAny) as
    | ((host: WidgetHost) => void | (() => void))
    | undefined
  if (typeof fn !== 'function') {
    throw new Error('组件入口必须导出 mountWidget(host) 或 mount(host)')
  }

  // —— ShadowRoot + 基础样式/主题变量 ——
  const shadow = opts.el.shadowRoot ?? opts.el.attachShadow({ mode: 'open' })
  shadow.replaceChildren()
  const style = document.createElement('style')
  style.textContent = hostCssVars() + HOST_CSS
  const rootEl = document.createElement('div')
  rootEl.className = 'pm-widget-root'
  shadow.append(style, rootEl)

  const perms = permissionSet(descriptor)
  const slotTags = slotTagsOf(deviceId, component.bind)
  const channels = new Map<string, SlotChannel>()
  for (const slot of new Set([
    ...Object.keys(descriptor.bindings ?? {}),
    ...Object.keys(slotTags),
  ])) {
    const ch = new SlotChannel(pkg)
    ch.setTags(slotTags[slot] ?? [])
    channels.set(slot, ch)
  }

  const state = {
    visible: true,
    disposed: false,
    config: (component.config ?? {}) as Record<string, unknown>,
  }

  function requireRead(): void {
    if (!canRead(perms)) {
      throw new Error(`组件未声明 tags:read 权限，槽位读取被拒绝`)
    }
  }

  function requireWrite(): void {
    if (!perms.has('tags:write')) {
      throw new Error('组件未声明 tags:write 权限，写入被拒绝')
    }
  }

  function channel(slot: string): SlotChannel {
    let ch = channels.get(slot)
    if (!ch) {
      ch = new SlotChannel(pkg)
      ch.setTags(slotTags[slot] ?? [])
      channels.set(slot, ch)
    }
    return ch
  }

  const host: WidgetHost = {
    el: rootEl,
    get mode() {
      return mode
    },
    get config() {
      return state.config
    },
    slots: {
      sub(slot, cb) {
        requireRead()
        const ch = channel(slot)
        ch.add(cb)
        if (state.visible) {
          ch.attach()
          const initial = ch.readAll()
          if (initial.length) {
            queueMicrotask(() => {
              if (!state.disposed && ch.listeners > 0) {
                try {
                  cb(initial)
                } catch (e) {
                  console.error('[widget] 首值回调异常', e)
                }
              }
            })
          }
        }
        return () => {
          if (ch.remove(cb)) ch.detach()
        }
      },
      read(slot) {
        requireRead()
        return channel(slot).readAll()
      },
      meta(slot) {
        requireRead()
        const tag = channel(slot).tags[0]
        return tag ? (resolveMeta(tag, pkg)?.meta ?? undefined) : undefined
      },
      async write(slot, value) {
        requireWrite()
        const decl = firstDecl(component.bind, slot)
        if (!decl) throw new Error(`槽位 ${slot} 未绑定数据点`)
        const target = resolveBoundTag(deviceId, decl)
        const hit = resolveMeta(target, pkg)
        if (!hit) throw new Error(`无法解析 ${target} 的点表元数据`)
        if (hit.point.access !== 'rw') throw new Error(`点 ${hit.point.name} 只读，写入被拒绝`)

        if (decl.bit === null || decl.bit === undefined) {
          validateWriteValue(hit.point, value)
          await writeTag(target, value)
          return
        }
        // 位写：读父点整字 → 改位 → 写回（带 scale/offset 时拒绝，避免写错原始位域）
        const parentTag = resolveBoundTag(deviceId, { point: decl.point ?? undefined })
        if (
          (hit.point.scale != null && hit.point.scale !== 1) ||
          (hit.point.offset != null && hit.point.offset !== 0)
        ) {
          throw new Error('该点带 scale/offset 换算，位写入请直接写父点')
        }
        const current = useTagsStore().get(parentTag)
        const num = primitiveOf(current?.value)
        if (typeof num !== 'number') throw new Error('还没有读到父点当前值，稍后再试')
        const bitOn = 'bool' in value ? value.bool : Number(primitiveOf(value)) !== 0
        const next = bitOn ? num | (1 << decl.bit) : num & ~(1 << decl.bit)
        await writeTag(parentTag, numericTagValue(hit.point, next))
      },
    },
    theme: {
      token: (name) => TOKENS[name] ?? '',
      mode: () => 'dark',
    },
    notify(message, kind = 'info') {
      if (!perms.has('notify')) {
        console.warn(`[widget ${descriptor.kind}] 未声明 notify 权限，通知被忽略`)
        return
      }
      const toast = useToastStore()
      if (kind === 'success') toast.success(message)
      else if (kind === 'error') toast.error(message)
      else toast.info(message)
    },
    log(...args) {
      console.info(`[widget ${descriptor.kind}]`, ...args)
    },
  }

  // 可见性门控：不可见时全部槽位退订
  let io: IntersectionObserver | null = null
  if (typeof IntersectionObserver !== 'undefined') {
    io = new IntersectionObserver(
      (entries) => {
        const visible = entries[0]?.isIntersecting ?? true
        if (visible === state.visible) return
        state.visible = visible
        for (const ch of channels.values()) {
          if (ch.listeners === 0) continue
          if (visible) ch.attach()
          else ch.detach()
        }
      },
      { rootMargin: '200px' },
    )
    io.observe(opts.el)
  }

  let cleanup: (() => void) | void
  try {
    cleanup = fn(host)
  } catch (e) {
    io?.disconnect()
    shadow.replaceChildren()
    throw e
  }

  const dispose = (): void => {
    if (state.disposed) return
    state.disposed = true
    io?.disconnect()
    io = null
    try {
      if (typeof cleanup === 'function') cleanup()
    } catch (e) {
      console.error(`[widget ${descriptor.kind}] 卸载异常`, e)
    }
    for (const ch of channels.values()) ch.detach()
    shadow.replaceChildren()
  }

  return {
    update(): boolean {
      if (state.disposed) return true
      const updateFn = mod.update
      if (typeof updateFn !== 'function') return false
      state.config = (component.config ?? {}) as Record<string, unknown>
      const next = slotTagsOf(deviceId, component.bind)
      for (const [slot, tags] of Object.entries(next)) {
        let ch = channels.get(slot)
        if (!ch) {
          ch = new SlotChannel(pkg)
          channels.set(slot, ch)
        }
        ch.setTags(tags)
      }
      try {
        ;(updateFn as (host: WidgetHost) => void)(host)
      } catch (e) {
        console.error(`[widget ${descriptor.kind}] update 异常`, e)
        return false
      }
      return true
    },
    dispose,
  }
}

function validateWriteValue(point: PointDto, value: TagValue): void {
  const p = primitiveOf(value)
  if (typeof p !== 'number') return
  if (point.min != null && p < point.min) {
    throw new Error(`写入值 ${p} 小于下限 ${point.min}`)
  }
  if (point.max != null && p > point.max) {
    throw new Error(`写入值 ${p} 大于上限 ${point.max}`)
  }
  const enumList = point.enum ?? []
  if (enumList.length && !enumList.some((e) => e.value === Math.round(p))) {
    throw new Error(`写入值 ${p} 不在枚举表内`)
  }
}

// —— v1 插件兼容挂载（light DOM + 受限 ctx）——

interface LegacyOptions extends MountExternalOptions {
  module: Record<string, unknown>
}

function mountLegacy(opts: LegacyOptions): WidgetInstance {
  const { descriptor, component, deviceId, pkg, mode, module } = opts
  const mountFn = module.mount as
    | ((el: HTMLElement, ctx: unknown, props: unknown) => void | (() => void))
    | undefined
  if (typeof mountFn !== 'function') {
    throw new Error('插件入口缺少 mount(el, ctx, props)')
  }

  const perms = permissionSet(descriptor)
  const allowed = new Set(
    Object.values(slotTagsOf(deviceId, component.bind)).flat().filter(Boolean),
  )
  const firstTag = [...allowed][0] ?? ''
  const meta = firstTag ? resolveMeta(firstTag, pkg)?.meta : undefined

  const offs = new Set<() => void>()
  const ctx = {
    plugin: {
      id: descriptor.providerId,
      name: descriptor.providerName,
      version: descriptor.version,
      permissions: [...perms],
    },
    tags: {
      subscribe(tags: string[], cb: (changes: TagChange[]) => void) {
        if (!canRead(perms)) throw new Error('插件未声明 tags:subscribe 权限')
        const requested = [...new Set(tags)].filter((t) => t && allowed.has(t))
        if (requested.length !== tags.length) {
          throw new Error('插件组件只能订阅该组件 bind 中声明的 tag')
        }
        const off = tagBus.subscribe(requested, cb)
        offs.add(off)
        const initial = requested
          .map((t) => useTagsStore().get(t))
          .filter((v): v is TagChange => Boolean(v))
        if (initial.length) {
          queueMicrotask(() => {
            try {
              cb(initial)
            } catch (e) {
              console.error('[widget] 首值回调异常', e)
            }
          })
        }
        return () => {
          offs.delete(off)
          off()
        }
      },
      read(tag: string) {
        return useTagsStore().get(tag)
      },
      async write(tag: string, value: TagValue) {
        if (!perms.has('tags:write')) throw new Error('插件未声明 tags:write 权限')
        if (!allowed.has(tag)) throw new Error('插件组件只能写该组件 bind 中声明的 tag')
        const hit = resolveMeta(tag, pkg)
        if (!hit) throw new Error(`无法解析 ${tag} 的点表元数据`)
        if (hit.point.access !== 'rw') throw new Error(`点 ${hit.point.name} 只读，写入被拒绝`)
        validateWriteValue(hit.point, value)
        await writeTag(tag, value)
      },
    },
    notify(message: string, kind: 'info' | 'success' | 'error' = 'info') {
      if (!perms.has('notify')) return
      const toast = useToastStore()
      if (kind === 'success') toast.success(message)
      else if (kind === 'error') toast.error(message)
      else toast.info(message)
    },
    theme: { get: () => 'dark' as const },
    log(...args: unknown[]) {
      console.info(`[widget ${descriptor.kind}]`, ...args)
    },
  }

  opts.el.replaceChildren()
  const cleanup = mountFn(opts.el, ctx, {
    kind: 'widget',
    widget: {
      id: component.id || descriptor.kind,
      title: component.title || descriptor.title,
      tag: firstTag,
      unit: meta?.unit ?? '',
      config: component.config ?? {},
    },
    editing: mode === 'edit',
  })

  let disposed = false
  return {
    update: () => false,
    dispose() {
      if (disposed) return
      disposed = true
      try {
        if (typeof cleanup === 'function') cleanup()
      } catch (e) {
        console.error(`[widget ${descriptor.kind}] 卸载异常`, e)
      }
      for (const off of offs) off()
      offs.clear()
      opts.el.replaceChildren()
    },
  }
}

/** 读取「开关类」写入的布尔语义（组件写位时用）。 */
export function bitValueOf(value: TagValue): boolean {
  return boolOf(value) ?? false
}
