//! 设备工作区页面：宿主组件目录 + 包默认/内置 fallback 页面。
//!
//! 组件 kind 白名单必须与 `crates/packaging/src/validate.rs` 的
//! `ALLOWED_DEVICE_PAGE_COMPONENTS` 保持一致（裸名是 v1 别名，统一归一化为 core:*）。

import type {
  BindingDecl,
  BindingSpec,
  DevicePageComponentDto,
  DevicePageDto,
  PackageDto,
  WidgetRect,
  WorkspaceDto,
} from '@/api/types'

export interface HostDeviceComponent {
  kind: string
  label: string
  icon: string
  description: string
}

export const HOST_DEVICE_COMPONENTS: HostDeviceComponent[] = [
  {
    kind: 'core:overview',
    label: '设备概览',
    icon: 'pulse',
    description: '在线状态、通道/从站、质量统计与最近更新',
  },
  {
    kind: 'core:dashboard',
    label: '仪表盘',
    icon: 'layout',
    description: '当前设备的组件画布（每个实例独立布局）',
  },
  {
    kind: 'core:tags',
    label: '实时点表',
    icon: 'grid',
    description: '当前设备的实时值、质量与写值',
  },
  {
    kind: 'core:settings',
    label: '参数设置',
    icon: 'settings',
    description: '设备实例名称、通道、从站与 overrides',
  },
  {
    kind: 'core:value',
    label: '数值卡',
    icon: 'activity',
    description: '绑定一个点位/位，显示工程量、质量与更新时间',
  },
  {
    kind: 'core:gauge',
    label: '仪表',
    icon: 'pulse',
    description: '绑定一个数值点，按 min/max 显示弧线刻度',
  },
  {
    kind: 'core:switch',
    label: '开关',
    icon: 'power',
    description: '绑定一个可写布尔点/位，点击即写入',
  },
  {
    kind: 'core:input',
    label: '输入',
    icon: 'edit',
    description: '按点表 min/max/step/enum 生成数值或枚举写入',
  },
  {
    kind: 'core:bits',
    label: '状态位',
    icon: 'grid',
    description: '绑定一个字点，按 bits 定义展示各位状态',
  },
  {
    kind: 'core:group',
    label: '组合容器',
    icon: 'layout',
    description: '把若干小组件按 12 列网格组合成一块（声明式，无代码）',
  },
]

export const ALLOWED_DEVICE_COMPONENT_KINDS = HOST_DEVICE_COMPONENTS.map((c) => c.kind)

const DEFAULT_DEVICE_PAGES: DevicePageDto[] = [
  {
    id: 'overview',
    title: '设备概览',
    icon: 'pulse',
    components: [{ kind: 'core:overview', title: '', config: null }],
  },
  {
    id: 'dashboard',
    title: '仪表盘',
    icon: 'layout',
    components: [{ kind: 'core:dashboard', title: '', config: null }],
  },
  {
    id: 'tags',
    title: '实时点表',
    icon: 'grid',
    components: [{ kind: 'core:tags', title: '', config: null }],
  },
  {
    id: 'settings',
    title: '参数设置',
    icon: 'settings',
    components: [{ kind: 'core:settings', title: '', config: null }],
  },
]

/** 深拷贝，避免编辑器原地修改包/默认数据。 */
export function clonePages(pages: DevicePageDto[]): DevicePageDto[] {
  return JSON.parse(JSON.stringify(pages)) as DevicePageDto[]
}

/** 用户覆盖 > 设备包声明 > 宿主预设组合页。 */
export function pagesForPackage(
  pkg?: PackageDto | null,
  saved?: WorkspaceDto,
): DevicePageDto[] {
  const source = saved?.pages?.length
    ? saved.pages
    : pkg?.ui?.pages?.length
      ? pkg.ui.pages
      : presetPagesForPackage(pkg)
  return clonePages(source)
}

interface PackCursor {
  x: number
  y: number
  rowH: number
}

function packRect(cur: PackCursor, w: number, h: number): WidgetRect {
  if (cur.x + w > 12) {
    cur.y += cur.rowH
    cur.x = 0
    cur.rowH = 0
  }
  const rect = { x: cur.x, y: cur.y, w, h }
  cur.x += w
  cur.rowH = Math.max(cur.rowH, h)
  return rect
}

/**
 * 包没有声明页面时，按点表生成**预设组合页**（P4）：
 * 概览/参数/状态位由 core:value / core:bits / core:input 组合而成，
 * 实时点表与设备设置保留为 `core:tags` / `core:settings` 复合组件。
 */
export function presetPagesForPackage(pkg?: PackageDto | null): DevicePageDto[] {
  if (!pkg || !pkg.points.length) return clonePages(DEFAULT_DEVICE_PAGES)

  const numeric = pkg.points.filter((p) => p.area !== 'coil' && p.area !== 'discrete')
  const writable = numeric.filter((p) => p.access === 'rw')
  const bitPoints = numeric.filter((p) => p.bits.length)

  const overviewWidgets: DevicePageDto['components'] = []
  const cursor: PackCursor = { x: 0, y: 0, rowH: 0 }
  for (const p of numeric.slice(0, 8)) {
    overviewWidgets.push({
      kind: 'core:value',
      title: p.name || p.id,
      bind: { value: { point: p.id } },
      rect: packRect(cursor, 3, 2),
      config: p.unit ? { unit: p.unit } : null,
    })
  }
  if (bitPoints.length) {
    overviewWidgets.push({
      kind: 'core:bits',
      title: bitPoints[0].name || bitPoints[0].id,
      bind: { value: { point: bitPoints[0].id } },
      rect: packRect(cursor, 4, 2),
      config: null,
    })
  }
  if (!overviewWidgets.length) {
    overviewWidgets.push({ kind: 'core:overview', title: '', config: null, rect: { x: 0, y: 0, w: 12, h: 6 } })
  }

  const pages: DevicePageDto[] = [
    { id: 'overview', title: '设备概览', icon: 'pulse', components: overviewWidgets },
  ]

  if (writable.length) {
    const params: DevicePageDto['components'] = []
    const pc: PackCursor = { x: 0, y: 0, rowH: 0 }
    for (const p of writable) {
      params.push({
        kind: 'core:input',
        title: p.name || p.id,
        bind: { value: { point: p.id } },
        rect: packRect(pc, 4, 2),
        config: null,
      })
    }
    pages.push({ id: 'params', title: '参数设置', icon: 'settings', components: params })
  }

  if (bitPoints.length) {
    const status: DevicePageDto['components'] = []
    const sc: PackCursor = { x: 0, y: 0, rowH: 0 }
    for (const p of bitPoints) {
      status.push({
        kind: 'core:bits',
        title: p.name || p.id,
        bind: { value: { point: p.id } },
        rect: packRect(sc, 6, 2),
        config: null,
      })
    }
    pages.push({ id: 'status', title: '状态位', icon: 'grid', components: status })
  }

  pages.push({
    id: 'tags',
    title: '实时点表',
    icon: 'grid',
    components: [{ kind: 'core:tags', title: '', config: null, rect: { x: 0, y: 0, w: 12, h: 10 } }],
  })
  pages.push({
    id: 'settings',
    title: '设备设置',
    icon: 'settings',
    components: [{ kind: 'core:settings', title: '', config: null, rect: { x: 0, y: 0, w: 12, h: 8 } }],
  })
  return pages
}

export function componentLabel(kind: string): string {
  const canonical = normalizeComponentKind(kind)
  return HOST_DEVICE_COMPONENTS.find((c) => c.kind === canonical)?.label ?? kind
}

/** 裸名是 v1 兼容别名：* → core:*。 */
export function normalizeComponentKind(kind: string): string {
  const bare = [
    'value',
    'gauge',
    'switch',
    'input',
    'bits',
    'group',
    'overview',
    'dashboard',
    'tags',
    'settings',
  ]
  return bare.includes(kind) ? `core:${kind}` : kind
}

const CORE_WIDGET_KINDS = [
  'core:value',
  'core:gauge',
  'core:switch',
  'core:input',
  'core:bits',
  'core:group',
]

/** 外部组件：`plugin:<插件id>:<组件id>` 或 `<包id>:<组件短名>`。 */
export function isExternalWidgetKind(kind: string): boolean {
  if (!kind.includes(':')) return false
  if (kind.startsWith('core:')) return false
  if (kind.startsWith('plugin:')) {
    const rest = kind.slice('plugin:'.length)
    const parts = rest.split(':')
    return parts.length === 2 && parts.every((p) => p.length > 0)
  }
  const parts = kind.split(':')
  return parts.length === 2 && parts.every((p) => p.length > 0)
}

/** 可放在 12 列网格/流式行里的小组件（核心小组件 + 外部 ESM 组件）。 */
export function isCompactWidgetKind(kind: string): boolean {
  return CORE_WIDGET_KINDS.includes(normalizeComponentKind(kind)) || isExternalWidgetKind(kind)
}

/** 组件的默认网格尺寸（编辑器的“网格布局”用）。 */
export function defaultWidgetSize(kind: string): { w: number; h: number } {
  if (isExternalWidgetKind(kind)) return { w: 6, h: 3 }
  switch (normalizeComponentKind(kind)) {
    case 'core:input':
    case 'core:bits':
      return { w: 4, h: 2 }
    case 'core:group':
      return { w: 6, h: 3 }
    case 'core:dashboard':
    case 'core:tags':
      return { w: 12, h: 10 }
    case 'core:overview':
      return { w: 12, h: 6 }
    case 'core:settings':
      return { w: 12, h: 8 }
    default:
      return { w: 3, h: 2 }
  }
}

/** 页面是否采用绝对网格布局（任一组件声明了 rect）。 */
export function pageUsesGrid(components: DevicePageComponentDto[]): boolean {
  return components.some((c) => c.rect)
}

export function isKnownComponent(kind: string): boolean {
  return (
    ALLOWED_DEVICE_COMPONENT_KINDS.includes(normalizeComponentKind(kind)) ||
    isExternalWidgetKind(kind)
  )
}

/** 取绑定声明列表（单绑定 → 长度 1；数组原样）。 */
export function bindingDecls(spec: BindingSpec | null | undefined): BindingDecl[] {
  if (!spec) return []
  return Array.isArray(spec) ? spec : [spec]
}

/**
 * 解析组件槽位绑定为完整 tag。
 *
 * - `{ tag: 'pm1.uab' }` → 绝对 tag（用户页面跨设备组态）
 * - `{ point: 'uab' }` + 当前设备 id → `pm1.uab`
 * - `{ point: 'status', bit: 2 }` → `pm1.status.b2`
 * - 数组 → 逐个解析，保持顺序
 */
export function resolveBoundTags(deviceId: string, spec: BindingSpec | null | undefined): string[] {
  return bindingDecls(spec)
    .map((bind) => resolveBoundTag(deviceId, bind))
    .filter(Boolean)
}

/** 单绑定解析（多绑定时取第一个；需要全部时用 [`resolveBoundTags`]）。 */
export function resolveBoundTag(
  deviceId: string,
  bind: BindingDecl | BindingSpec | null | undefined,
): string {
  const first = Array.isArray(bind) ? bind[0] : bind
  if (!first) return ''
  const absolute = typeof first.tag === 'string' ? first.tag.trim() : ''
  if (absolute) return absolute
  const point = typeof first.point === 'string' ? first.point.trim() : ''
  if (!point) return ''
  const bit = typeof first.bit === 'number' ? `.b${first.bit}` : ''
  return `${deviceId}.${point}${bit}`
}

/** 取组件某个槽位的绑定声明。 */
export function slotBind(
  bind: Record<string, BindingSpec> | null | undefined,
  slot: string,
): BindingSpec | undefined {
  return bind?.[slot]
}
