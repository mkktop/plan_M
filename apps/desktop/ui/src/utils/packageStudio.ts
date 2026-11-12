//! 包编辑器（P4）：表单草稿 → manifest.json / points.json 的纯函数层。
//!
//! 用户全程只填表单；这里负责生成 v2 页面（widgets[]）、点表与校验所需的 JSON。

import type { Access, ByteOrder, DataType, RegArea, WidgetRect } from '@/api/types'

export interface DraftBit {
  bit: number | null
  name: string
  on: string
  off: string
}

export interface DraftEnum {
  value: number | null
  label: string
}

export interface DraftPoint {
  /** 编辑行稳定 key（不写入 JSON） */
  key: string
  id: string
  name: string
  address: string
  byteOrder: ByteOrder
  type: DataType
  access: Access
  scale: number | null
  offset: number | null
  unit: string
  pollGroup: string
  min: number | null
  max: number | null
  step: number | null
  bits: DraftBit[]
  enum: DraftEnum[]
}

export interface DraftWidgetConfig {
  unit?: string
  decimals?: number | null
  min?: number | null
  max?: number | null
  onLabel?: string
  offLabel?: string
}

export interface DraftGroupItem {
  kind: string
  title: string
  point: string
  bit: number | null
  rect: WidgetRect
}

export interface DraftWidget {
  id: string
  kind: string
  title: string
  rect: WidgetRect
  /** value 槽位绑定的点 */
  point: string
  /** 位绑定（仅单字点） */
  bit: number | null
  /** 多点槽位（趋势图等） */
  points: string[]
  config: DraftWidgetConfig
  /** core:group 的子组件 */
  groupItems: DraftGroupItem[]
}

export interface DraftPage {
  id: string
  title: string
  icon: string
  widgets: DraftWidget[]
}

export interface DraftPollGroup {
  name: string
  intervalMs: number | null
}

export interface PackageDraft {
  meta: {
    id: string
    name: string
    vendor: string
    version: string
    description: string
    driver: string
  }
  pollGroups: DraftPollGroup[]
  points: DraftPoint[]
  pages: DraftPage[]
}

export const CORE_WIDGET_OPTIONS = [
  { label: '数值卡 · core:value', value: 'core:value' },
  { label: '仪表 · core:gauge', value: 'core:gauge' },
  { label: '开关 · core:switch', value: 'core:switch' },
  { label: '输入 · core:input', value: 'core:input' },
  { label: '状态位 · core:bits', value: 'core:bits' },
  { label: '组合容器 · core:group', value: 'core:group' },
]

export const POINT_TYPES: DataType[] = ['u16', 'i16', 'u32', 'i32', 'u64', 'f32', 'f64', 'bcd16', 'bcd32']

let seq = 0
function nextKey(prefix: string): string {
  seq += 1
  return `${prefix}-${Date.now().toString(36)}-${seq.toString(36)}`
}

/** 寄存器地址 → 数据区（0x 线圈 / 1x 离散 / 3x 输入 / 4x 保持）。 */
export function areaOfRegister(address: string): RegArea | null {
  const r = address.trim().toUpperCase()
  if (/^C:/.test(r)) return 'coil'
  if (/^DR:/.test(r)) return 'discrete'
  if (/^IR:/.test(r)) return 'input'
  if (/^HR:/.test(r)) return 'holding'
  if (/^0X/.test(r)) return 'coil'
  if (/^1X/.test(r)) return 'discrete'
  if (/^3X/.test(r)) return 'input'
  if (/^4X/.test(r)) return 'holding'
  const first = r.charAt(0)
  if (!/^\d+$/.test(r)) return null
  if (first === '0') return 'coil'
  if (first === '1') return 'discrete'
  if (first === '3') return 'input'
  if (first === '4') return 'holding'
  return null
}

export function isBitArea(address: string): boolean {
  const area = areaOfRegister(address)
  return area === 'coil' || area === 'discrete'
}

export function emptyDraft(): PackageDraft {
  return {
    meta: { id: '', name: '', vendor: '', version: '0.1.0', description: '', driver: 'modbus' },
    pollGroups: [{ name: 'fast', intervalMs: 1000 }],
    points: [],
    pages: [{ id: 'overview', title: '设备概览', icon: 'pulse', widgets: [] }],
  }
}

export function newPoint(index: number): DraftPoint {
  return {
    key: nextKey('p'),
    id: `point${index + 1}`,
    name: '',
    address: String(40101 + index * 1),
    byteOrder: 'abcd',
    type: 'u16',
    access: 'r',
    scale: null,
    offset: null,
    unit: '',
    pollGroup: 'fast',
    min: null,
    max: null,
    step: null,
    bits: [],
    enum: [],
  }
}

export function newPage(index: number): DraftPage {
  return {
    id: `page-${Date.now().toString(36)}-${index}`,
    title: `新页面 ${index + 1}`,
    icon: 'grid',
    widgets: [],
  }
}

export function newWidget(kind: string, point = ''): DraftWidget {
  const size =
    kind === 'core:group'
      ? { w: 6, h: 3 }
      : kind === 'core:bits' || kind === 'core:input'
        ? { w: 4, h: 2 }
        : { w: 3, h: 2 }
  return {
    id: nextKey('w'),
    kind,
    title: '',
    rect: { x: 0, y: 0, ...size },
    point,
    bit: null,
    points: [],
    config: {},
    groupItems:
      kind === 'core:group'
        ? [
            { kind: 'core:value', title: '数值', point, bit: null, rect: { x: 0, y: 0, w: 12, h: 2 } },
            { kind: 'core:value', title: '数值 2', point: '', bit: null, rect: { x: 0, y: 2, w: 12, h: 2 } },
          ]
        : [],
  }
}

/** 把组件放到页面上（简单行优先：接在最后一行下面）。 */
export function appendRect(page: DraftPage, w: DraftWidget): WidgetRect {
  const y = page.widgets.reduce((max, x) => Math.max(max, x.rect.y + x.rect.h), 0)
  return { x: 0, y, w: w.rect.w, h: w.rect.h }
}

// —— 生成 JSON ——

function bindingOf(point: string, bit: number | null): Record<string, unknown> | null {
  if (!point.trim()) return null
  return bit === null || bit === undefined
    ? { point: point.trim() }
    : { point: point.trim(), bit }
}

function widgetConfig(w: DraftWidget): Record<string, unknown> | null {
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(w.config)) {
    if (v === '' || v === null || v === undefined) continue
    out[k] = v
  }
  if (w.kind === 'core:group' && w.groupItems.length) {
    out.items = w.groupItems.map((item) => ({
      kind: item.kind,
      title: item.title,
      rect: item.rect,
      ...(bindingOf(item.point, item.bit) ? { bind: { value: bindingOf(item.point, item.bit) } } : {}),
    }))
  }
  return Object.keys(out).length ? out : null
}

function widgetBind(w: DraftWidget): Record<string, unknown> | null {
  if (w.kind === 'core:group') return null
  if (w.points.length) {
    const many = w.points.filter(Boolean).map((p) => ({ point: p }))
    return many.length ? { series: many } : null
  }
  const one = bindingOf(w.point, w.bit)
  return one ? { value: one } : null
}

export function buildManifest(draft: PackageDraft): Record<string, unknown> {
  return {
    schemaVersion: 2,
    id: draft.meta.id.trim(),
    name: draft.meta.name.trim(),
    vendor: draft.meta.vendor.trim(),
    description: draft.meta.description.trim(),
    version: draft.meta.version.trim() || '0.1.0',
    protocol: {
      driver: draft.meta.driver,
      transports: ['serial', 'tcp', 'sim'],
      params: { framing: 'auto', byteOrder: 'abcd' },
    },
    ui: {
      pages: draft.pages.map((page) => ({
        schemaVersion: 2,
        id: page.id,
        title: page.title.trim() || page.id,
        icon: page.icon,
        layout: { cols: 12, rowHeight: 72, gap: 12 },
        widgets: page.widgets.map((w) => ({
          id: w.id,
          kind: w.kind,
          title: w.title.trim(),
          rect: w.rect,
          ...(widgetBind(w) ? { bind: widgetBind(w) } : {}),
          ...(widgetConfig(w) ? { config: widgetConfig(w) } : {}),
        })),
      })),
    },
  }
}

function bitsJson(bits: DraftBit[]): Record<string, unknown> | undefined {
  if (!bits.length) return undefined
  const out: Record<string, unknown> = {}
  for (const b of bits) {
    const bit = Math.max(0, Math.min(15, Math.round(b.bit ?? 0)))
    out[String(bit)] = {
      name: b.name.trim() || `位 ${bit}`,
      ...(b.on.trim() ? { on: b.on.trim() } : {}),
      ...(b.off.trim() ? { off: b.off.trim() } : {}),
    }
  }
  return out
}

export function buildPoints(draft: PackageDraft): Record<string, unknown> {
  return {
    pollGroups: draft.pollGroups
      .filter((g) => g.name.trim())
      .map((g) => ({
        name: g.name.trim(),
        intervalMs: Math.max(50, Math.round(g.intervalMs ?? 1000)),
      })),
    points: draft.points
      .filter((p) => p.id.trim())
      .map((p) => {
        const bitArea = isBitArea(p.address)
        return {
          id: p.id.trim(),
          name: p.name.trim() || p.id.trim(),
          address: p.address.trim(),
          ...(bitArea ? {} : { type: p.type, byteOrder: p.byteOrder }),
          access: p.access,
          ...(!bitArea && p.scale != null ? { scale: p.scale } : {}),
          ...(!bitArea && p.offset != null ? { offset: p.offset } : {}),
          ...(p.unit.trim() ? { unit: p.unit.trim() } : {}),
          ...(p.pollGroup.trim() ? { pollGroup: p.pollGroup.trim() } : {}),
          ...(p.min != null ? { min: p.min } : {}),
          ...(p.max != null ? { max: p.max } : {}),
          ...(p.step != null ? { step: p.step } : {}),
          ...(bitsJson(p.bits) ? { bits: bitsJson(p.bits) } : {}),
          ...(p.enum.length
            ? {
                enum: p.enum.map((e) => ({
                  value: Math.round(e.value ?? 0),
                  label: e.label.trim(),
                })),
              }
            : {}),
        }
      }),
  }
}

// —— 预设组合页面（P4：用点表生成页面，不写 JSON）——

interface RowPacker {
  x: number
  y: number
  rowH: number
}

function place(packer: RowPacker, w: number, h: number): WidgetRect {
  if (packer.x + w > 12) {
    packer.y += packer.rowH
    packer.x = 0
    packer.rowH = 0
  }
  const rect = { x: packer.x, y: packer.y, w, h }
  packer.x += w
  packer.rowH = Math.max(packer.rowH, h)
  return rect
}

function pointLabel(p: DraftPoint): string {
  return p.name.trim() || p.id.trim()
}

/**
 * 从点表生成预设组合页面：
 * - 设备概览：数值卡（数值点）+ 状态位（首个状态字）
 * - 参数设置：可写点 → 输入组件
 * - 状态位：所有带 bits 的字点
 * - 实时点表：core:tags（代码型复合组件，作为兜底页）
 */
export function presetPages(draft: PackageDraft): DraftPage[] {
  const numeric = draft.points.filter((p) => p.id.trim() && !isBitArea(p.address))
  const writable = numeric.filter((p) => p.access === 'rw')
  const bitPoints = numeric.filter((p) => p.bits.length)

  const overview: DraftPage = { id: 'overview', title: '设备概览', icon: 'pulse', widgets: [] }
  const pack: RowPacker = { x: 0, y: 0, rowH: 0 }
  for (const p of numeric.slice(0, 8)) {
    const w = newWidget('core:value', p.id)
    w.title = pointLabel(p)
    w.rect = place(pack, 3, 2)
    overview.widgets.push(w)
  }
  if (bitPoints.length) {
    const w = newWidget('core:bits', bitPoints[0].id)
    w.title = pointLabel(bitPoints[0])
    w.rect = place(pack, 4, 2)
    overview.widgets.push(w)
  }
  if (!overview.widgets.length) {
    overview.widgets.push({ ...newWidget('core:value'), title: '数值' })
  }

  const params: DraftPage = { id: 'params', title: '参数设置', icon: 'settings', widgets: [] }
  const pack2: RowPacker = { x: 0, y: 0, rowH: 0 }
  for (const p of writable) {
    const w = newWidget('core:input', p.id)
    w.title = pointLabel(p)
    w.rect = place(pack2, 4, 2)
    params.widgets.push(w)
  }
  if (!params.widgets.length) {
    const w = newWidget('core:input')
    w.title = '可写参数'
    params.widgets.push(w)
  }

  const status: DraftPage = { id: 'status', title: '状态位', icon: 'grid', widgets: [] }
  const pack3: RowPacker = { x: 0, y: 0, rowH: 0 }
  for (const p of bitPoints) {
    const w = newWidget('core:bits', p.id)
    w.title = pointLabel(p)
    w.rect = place(pack3, 6, 2)
    status.widgets.push(w)
  }
  if (!status.widgets.length) {
    const w = newWidget('core:bits')
    w.title = '状态字'
    status.widgets.push(w)
  }

  const tags: DraftPage = {
    id: 'tags',
    title: '实时点表',
    icon: 'grid',
    widgets: [
      {
        ...newWidget('core:tags'),
        kind: 'core:tags',
        title: '全部点位',
        rect: { x: 0, y: 0, w: 12, h: 10 },
      },
    ],
  }
  const settings: DraftPage = {
    id: 'settings',
    title: '设备设置',
    icon: 'settings',
    widgets: [
      {
        ...newWidget('core:settings'),
        kind: 'core:settings',
        title: '实例参数',
        rect: { x: 0, y: 0, w: 12, h: 8 },
      },
    ],
  }
  return [overview, params, status, tags, settings]
}

// —— 本地草稿（关闭弹窗不丢工作）——

const STORAGE_KEY = 'hmi.packageDraft.v1'

export function loadDraft(): PackageDraft | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as PackageDraft
    if (!parsed?.meta || !Array.isArray(parsed.points) || !Array.isArray(parsed.pages)) return null
    return parsed
  } catch {
    return null
  }
}

export function saveDraft(draft: PackageDraft): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(draft))
  } catch {
    // localStorage 满/禁用时忽略：草稿仍在内存里
  }
}

export function clearDraft(): void {
  try {
    localStorage.removeItem(STORAGE_KEY)
  } catch {
    /* ignore */
  }
}
