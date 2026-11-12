//! 设备工作区页面：宿主组件目录 + 包默认/内置 fallback 页面。
//!
//! 组件 kind 白名单必须与 `crates/packaging/src/validate.rs` 的
//! `ALLOWED_DEVICE_PAGE_COMPONENTS` 保持一致。

import type {
  BindingDecl,
  DevicePageComponentDto,
  DevicePageDto,
  PackageDto,
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
    kind: 'overview',
    label: '设备概览',
    icon: 'pulse',
    description: '在线状态、通道/从站、质量统计与最近更新',
  },
  {
    kind: 'dashboard',
    label: '仪表盘',
    icon: 'layout',
    description: '当前设备的组件画布（每个实例独立布局）',
  },
  {
    kind: 'tags',
    label: '实时点表',
    icon: 'grid',
    description: '当前设备的实时值、质量与写值',
  },
  {
    kind: 'settings',
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
]

export const ALLOWED_DEVICE_COMPONENT_KINDS = HOST_DEVICE_COMPONENTS.map((c) => c.kind)

const DEFAULT_DEVICE_PAGES: DevicePageDto[] = [
  {
    id: 'overview',
    title: '设备概览',
    icon: 'pulse',
    components: [{ kind: 'overview', title: '', config: null }],
  },
  {
    id: 'dashboard',
    title: '仪表盘',
    icon: 'layout',
    components: [{ kind: 'dashboard', title: '', config: null }],
  },
  {
    id: 'tags',
    title: '实时点表',
    icon: 'grid',
    components: [{ kind: 'tags', title: '', config: null }],
  },
  {
    id: 'settings',
    title: '参数设置',
    icon: 'settings',
    components: [{ kind: 'settings', title: '', config: null }],
  },
]

/** 深拷贝，避免编辑器原地修改包/默认数据。 */
export function clonePages(pages: DevicePageDto[]): DevicePageDto[] {
  return JSON.parse(JSON.stringify(pages)) as DevicePageDto[]
}

/** 用户覆盖 > 设备包声明 > 宿主内置四页。 */
export function pagesForPackage(
  pkg?: PackageDto | null,
  saved?: WorkspaceDto,
): DevicePageDto[] {
  const source =
    saved?.pages?.length
      ? saved.pages
      : pkg?.ui?.pages?.length
        ? pkg.ui.pages
        : DEFAULT_DEVICE_PAGES
  return clonePages(source)
}

export function componentLabel(kind: string): string {
  const canonical = normalizeComponentKind(kind)
  return HOST_DEVICE_COMPONENTS.find((c) => c.kind === canonical)?.label ?? kind
}

/** 裸名是 v1 兼容别名：value/gauge/switch/input/bits → core:*。 */
export function normalizeComponentKind(kind: string): string {
  return ['value', 'gauge', 'switch', 'input', 'bits'].includes(kind)
    ? `core:${kind}`
    : kind
}

/** 可放在 12 列网格里的小组件（含旧页面流式声明）。 */
export const COMPACT_WIDGET_KINDS = [
  'value',
  'gauge',
  'switch',
  'input',
  'bits',
  'core:value',
  'core:gauge',
  'core:switch',
  'core:input',
  'core:bits',
]

export function isCompactWidgetKind(kind: string): boolean {
  return COMPACT_WIDGET_KINDS.includes(kind)
}

/** 组件的默认网格尺寸（编辑器的“网格布局”用）。 */
export function defaultWidgetSize(kind: string): { w: number; h: number } {
  switch (normalizeComponentKind(kind)) {
    case 'core:input':
    case 'core:bits':
      return { w: 4, h: 2 }
    case 'dashboard':
    case 'tags':
      return { w: 12, h: 10 }
    case 'overview':
      return { w: 12, h: 6 }
    case 'settings':
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
  return ALLOWED_DEVICE_COMPONENT_KINDS.includes(normalizeComponentKind(kind))
}

/**
 * 解析组件槽位绑定为完整 tag。
 *
 * - `{ tag: 'pm1.uab' }` → 绝对 tag（用户页面跨设备组态）
 * - `{ point: 'uab' }` + 当前设备 id → `pm1.uab`
 * - `{ point: 'status', bit: 2 }` → `pm1.status.b2`
 */
export function resolveBoundTag(deviceId: string, bind: BindingDecl | null | undefined): string {
  if (!bind) return ''
  const absolute = typeof bind.tag === 'string' ? bind.tag.trim() : ''
  if (absolute) return absolute
  const point = typeof bind.point === 'string' ? bind.point.trim() : ''
  if (!point) return ''
  const bit = typeof bind.bit === 'number' ? `.b${bind.bit}` : ''
  return `${deviceId}.${point}${bit}`
}

/** 取组件某个槽位的绑定声明。 */
export function slotBind(
  bind: Record<string, BindingDecl> | null | undefined,
  slot: string,
): BindingDecl | undefined {
  return bind?.[slot]
}
