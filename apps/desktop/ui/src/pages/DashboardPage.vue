<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, ref, watch } from 'vue'
import { NButton, NInput, NInputNumber, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import EmptyState from '@/components/EmptyState.vue'
import ModalShell from '@/components/ModalShell.vue'
import PageHeader from '@/components/PageHeader.vue'
import QualityPill from '@/components/QualityPill.vue'
import { gateway } from '@/api/ws'
import { useConnectionStore } from '@/stores/connection'
import { useDashboardsStore } from '@/stores/dashboards'
import { useProjectStore } from '@/stores/project'
import { useTagsStore } from '@/stores/tags'
import { errorMessage, useToastStore } from '@/stores/toast'
import { boolOf, formatNumber, primitiveOf, qualityClass, valueText } from '@/utils/format'
import type {
  BitDto,
  DashboardDto,
  DeviceDto,
  PointDto,
  TagValue,
  WidgetDto,
  WidgetLayout,
} from '@/api/types'

const GAP = 12
const ROW_H = 64
const MAX_ROWS = 500

const dashStore = useDashboardsStore()
const project = useProjectStore()
const tagStore = useTagsStore()
const conn = useConnectionStore()
const toast = useToastStore()

const currentId = ref('')
const editing = ref(false)
const canvasRef = ref<HTMLElement | null>(null)
const draggingId = ref('')
const saveError = ref('')
const lastSavedAt = ref(0)

const current = computed<DashboardDto | null>(() => {
  if (!dashStore.list.length) return null
  return dashStore.list.find((d) => d.id === currentId.value) ?? dashStore.list[0]
})

const dashboardOptions = computed(() =>
  dashStore.list.map((d) => ({ label: d.name, value: d.id })),
)

watch(
  () => dashStore.list.map((d) => `${d.id}:${d.name}`).join('|'),
  () => {
    if (!dashStore.list.some((d) => d.id === currentId.value)) {
      currentId.value = dashStore.list[0]?.id ?? ''
    }
  },
  { immediate: true },
)

// —— 加载（页面进入时可能还没握手，连接成功后再取一次）——

async function ensureLoaded(force = false): Promise<void> {
  if (!conn.connected) return
  try {
    await dashStore.load(force)
  } catch (e) {
    saveError.value = errorMessage(e)
  }
}

watch(
  () => conn.connected,
  (ok) => {
    if (ok) void ensureLoaded()
  },
  { immediate: true },
)

// —— 订阅当前看板绑定的 tag（引用计数由客户端处理）——

const watchedTags = computed(() =>
  (current.value?.widgets ?? []).map((w) => w.tag).filter((t): t is string => !!t),
)

let unsubscribe: (() => void) | null = null

watch(
  watchedTags,
  (list) => {
    unsubscribe?.()
    unsubscribe = list.length ? tagStore.subscribe([...new Set(list)]) : null
  },
  { immediate: true },
)

// —— 保存（显式变更后防抖；同一时间只发一个保存请求）——

let saveTimer: number | undefined
let saveQueued = false

function cloneDashboard(d: DashboardDto): DashboardDto {
  return JSON.parse(JSON.stringify(d)) as DashboardDto
}

async function saveNow(): Promise<void> {
  if (saveTimer) {
    window.clearTimeout(saveTimer)
    saveTimer = undefined
  }
  const d = current.value
  if (!d) return
  if (dashStore.saving) {
    saveQueued = true
    return
  }
  saveError.value = ''
  try {
    await dashStore.save(cloneDashboard(d))
    lastSavedAt.value = Date.now()
  } catch (e) {
    saveError.value = errorMessage(e)
  } finally {
    if (saveQueued) {
      saveQueued = false
      void saveNow()
    }
  }
}

function scheduleSave(): void {
  if (saveTimer) window.clearTimeout(saveTimer)
  saveTimer = window.setTimeout(() => {
    saveTimer = undefined
    void saveNow()
  }, 600)
}

const saveStateText = computed(() => {
  if (dashStore.saving) return '保存中…'
  if (saveError.value) return '保存失败'
  if (lastSavedAt.value) return '已保存'
  return ''
})

// —— 看板操作 ——

async function newDashboard(): Promise<void> {
  const id = `dash-${Date.now().toString(36)}`
  const d: DashboardDto = {
    id,
    name: `仪表盘 ${dashStore.list.length + 1}`,
    cols: 12,
    widgets: [],
  }
  try {
    await dashStore.save(d)
    currentId.value = d.id
    editing.value = true
    toast.success('已新建仪表盘，添加组件开始组态')
  } catch (e) {
    toast.error(errorMessage(e))
  }
}

const nameDraft = ref('')
watch(
  current,
  (d) => {
    nameDraft.value = d?.name ?? ''
  },
  { immediate: true },
)

function commitName(): void {
  const d = current.value
  if (!d) return
  const v = nameDraft.value.trim()
  if (!v) {
    nameDraft.value = d.name
    return
  }
  if (v === d.name) return
  d.name = v
  void saveNow()
}

const dashboardToDelete = ref<DashboardDto | null>(null)
const deletingDashboard = ref(false)

async function doDeleteDashboard(): Promise<void> {
  const d = dashboardToDelete.value
  if (!d) return
  deletingDashboard.value = true
  try {
    await dashStore.remove(d.id)
    dashboardToDelete.value = null
    toast.success(`仪表盘「${d.name}」已删除`)
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    deletingDashboard.value = false
  }
}

// —— tag / 点表解析（用于标题、单位与写权限；找不到时按未绑定处理）——

interface ResolvedPoint {
  device: DeviceDto
  point: PointDto
  bit?: BitDto
}

function resolvePoint(tag: string): ResolvedPoint | null {
  if (!tag) return null
  const [deviceId, pointId, bitId] = tag.split('.')
  const device = project.devices.find((d) => d.id === deviceId)
  if (!device) return null
  const pkg = project.packageById.get(device.package)
  const point = pkg?.points.find((p) => p.id === pointId)
  if (!point) return null
  const bit =
    bitId && bitId.startsWith('b')
      ? point.bits.find((b) => `b${b.bit}` === bitId)
      : undefined
  return { device, point, bit }
}

function canWrite(tag: string): boolean {
  const r = resolvePoint(tag)
  return !!r && !r.bit && r.point.access === 'rw'
}

const tagOptions = computed(() => {
  const out: { label: string; value: string; unit: string }[] = []
  for (const d of project.devices) {
    const pkg = project.packageById.get(d.package)
    if (!pkg) continue
    const devName = d.name || d.id
    for (const p of pkg.points) {
      out.push({
        label: `${devName} · ${p.name}${p.unit ? `（${p.unit}）` : ''}`,
        value: `${d.id}.${p.id}`,
        unit: p.unit,
      })
      for (const b of p.bits) {
        out.push({
          label: `${devName} · ${p.name} / ${b.name}`,
          value: `${d.id}.${p.id}.b${b.bit}`,
          unit: '',
        })
      }
    }
  }
  return out
})

// —— 组件渲染辅助 ——

function tagChange(tag: string) {
  return tagStore.get(tag)
}

function cfgNumber(w: WidgetDto, key: string, fallback: number): number {
  const v = w.config?.[key]
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback
}

function cfgString(w: WidgetDto, key: string, fallback: string): string {
  const v = w.config?.[key]
  return typeof v === 'string' ? v : fallback
}

function numberText(w: WidgetDto): string {
  const p = primitiveOf(tagChange(w.tag)?.value)
  if (typeof p !== 'number') return valueText(tagChange(w.tag)?.value)
  const decimals = Math.max(0, Math.min(6, cfgNumber(w, 'decimals', 1)))
  return formatNumber(p, decimals)
}

function gaugePct(w: WidgetDto): number {
  const p = primitiveOf(tagChange(w.tag)?.value)
  if (typeof p !== 'number') return 0
  const min = cfgNumber(w, 'min', 0)
  const max = cfgNumber(w, 'max', 100)
  if (max === min) return 0
  return Math.max(0, Math.min(1, ((p - min) / (max - min)) * 100))
}

function gaugeDash(w: WidgetDto): string {
  return `${gaugePct(w)} 100`
}

function switchOn(w: WidgetDto): boolean {
  return boolOf(tagChange(w.tag)?.value) ?? false
}

const writing = reactive(new Set<string>())

async function toggleSwitch(w: WidgetDto): Promise<void> {
  if (!canWrite(w.tag) || writing.has(w.tag)) return
  const r = resolvePoint(w.tag)
  const raw = tagChange(w.tag)?.value
  const on = boolOf(raw)
  if (!r || on === undefined) return
  const value: TagValue =
    r.point.area === 'coil' ? { bool: !on } : { i64: on ? 0 : 1 }
  writing.add(w.tag)
  try {
    await gateway.request({ type: 'writeTag', tag: w.tag, value }, ['commandOk', 'error'], 8000)
  } catch (e) {
    toast.error(`写入失败：${errorMessage(e)}`)
  } finally {
    writing.delete(w.tag)
  }
}

// —— 网格布局：拖动 / 缩放（编辑模式）——

const cols = computed(() => current.value?.cols ?? 12)

const gridStyle = computed(() => ({
  gridTemplateColumns: `repeat(${cols.value}, minmax(0, 1fr))`,
  gridAutoRows: `${ROW_H}px`,
  gap: `${GAP}px`,
}))

function widgetStyle(w: WidgetDto) {
  return {
    gridColumn: `${w.layout.x + 1} / span ${w.layout.w}`,
    gridRow: `${w.layout.y + 1} / span ${w.layout.h}`,
  }
}

function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v))
}

function minW(kind: string): number {
  return kind === 'gauge' ? 3 : 2
}

function minH(kind: string): number {
  return kind === 'gauge' ? 3 : 2
}

interface DragState {
  widget: WidgetDto
  mode: 'move' | 'resize'
  startX: number
  startY: number
  origin: WidgetLayout
  cellW: number
  cellH: number
}

let dragState: DragState | null = null

function beginDrag(ev: Event, w: WidgetDto, mode: 'move' | 'resize'): void {
  if (!editing.value || !canvasRef.value) return
  const e = ev as PointerEvent
  e.preventDefault()
  e.stopPropagation()
  const cellW = (canvasRef.value.clientWidth - GAP * (cols.value - 1)) / cols.value
  dragState = {
    widget: w,
    mode,
    startX: e.clientX,
    startY: e.clientY,
    origin: { ...w.layout },
    cellW,
    cellH: ROW_H + GAP,
  }
  draggingId.value = w.id
  window.addEventListener('pointermove', onDragMove)
  window.addEventListener('pointerup', endDrag, { once: true })
}

function onDragMove(ev: PointerEvent): void {
  const st = dragState
  if (!st) return
  const dx = Math.round((ev.clientX - st.startX) / (st.cellW + GAP))
  const dy = Math.round((ev.clientY - st.startY) / st.cellH)
  const w = st.widget
  const d = current.value
  if (!d) return
  const next: WidgetLayout = { ...w.layout }
  if (st.mode === 'move') {
    next.x = clamp(st.origin.x + dx, 0, cols.value - w.layout.w)
    next.y = clamp(st.origin.y + dy, 0, MAX_ROWS - w.layout.h)
  } else {
    next.w = clamp(st.origin.w + dx, minW(w.kind), cols.value - w.layout.x)
    next.h = clamp(st.origin.h + dy, minH(w.kind), 12)
  }
  // 网格布局不允许重叠：撞到其他组件时停在上一个合法位置。
  if (d.widgets.some((o) => o.id !== w.id && overlaps(o.layout, next))) return
  w.layout = next
}

function endDrag(): void {
  window.removeEventListener('pointermove', onDragMove)
  dragState = null
  draggingId.value = ''
  scheduleSave()
}

function overlaps(a: WidgetLayout, b: WidgetLayout): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

function findSlot(d: DashboardDto, w: number, h: number, excludeId?: string): WidgetLayout {
  for (let y = 0; y < 200; y++) {
    for (let x = 0; x + w <= d.cols; x++) {
      const cand: WidgetLayout = { x, y, w, h }
      if (!d.widgets.some((o) => o.id !== excludeId && overlaps(o.layout, cand))) return cand
    }
  }
  return { x: 0, y: 0, w, h }
}

onBeforeUnmount(() => {
  window.removeEventListener('pointermove', onDragMove)
  unsubscribe?.()
  if (saveTimer) window.clearTimeout(saveTimer)
})

// —— 组件编辑对话框 ——

const showWidgetModal = ref(false)
const editingWidgetId = ref<string | null>(null)
const widgetForm = reactive({
  kind: 'value',
  title: '',
  tag: '',
  unit: '',
  min: 0,
  max: 100,
  decimals: 1,
  onLabel: 'ON',
  offLabel: 'OFF',
})

const kindOptions = [
  { label: '数值卡', value: 'value' },
  { label: '开关', value: 'switch' },
  { label: '仪表', value: 'gauge' },
]

function openAddWidget(): void {
  editingWidgetId.value = null
  Object.assign(widgetForm, {
    kind: 'value',
    title: '',
    tag: '',
    unit: '',
    min: 0,
    max: 100,
    decimals: 1,
    onLabel: 'ON',
    offLabel: 'OFF',
  })
  showWidgetModal.value = true
}

function openWidgetSettings(w: WidgetDto): void {
  editingWidgetId.value = w.id
  widgetForm.kind = w.kind
  widgetForm.title = w.title
  widgetForm.tag = w.tag
  widgetForm.unit = w.unit
  widgetForm.min = cfgNumber(w, 'min', 0)
  widgetForm.max = cfgNumber(w, 'max', 100)
  widgetForm.decimals = cfgNumber(w, 'decimals', 1)
  widgetForm.onLabel = cfgString(w, 'onLabel', 'ON')
  widgetForm.offLabel = cfgString(w, 'offLabel', 'OFF')
  showWidgetModal.value = true
}

function onTagPicked(tag: string): void {
  // 注意：不能与 v-model:value 同时写 @update:value（Vue 会覆盖同一个监听），
  // 所以这个 select 用 :value + @update:value，由本函数负责回填。
  widgetForm.tag = tag
  const r = resolvePoint(tag)
  if (!r) return
  const defaultTitle = r.bit ? `${r.point.name} · ${r.bit.name}` : r.point.name
  if (!widgetForm.title.trim()) widgetForm.title = defaultTitle
  widgetForm.unit = r.point.unit
}

function configOfForm(): Record<string, unknown> {
  if (widgetForm.kind === 'gauge') {
    return { min: widgetForm.min, max: widgetForm.max }
  }
  if (widgetForm.kind === 'switch') {
    return { onLabel: widgetForm.onLabel, offLabel: widgetForm.offLabel }
  }
  return { decimals: widgetForm.decimals }
}

function submitWidget(): void {
  const d = current.value
  if (!d) return
  if (!widgetForm.tag) {
    toast.error('请选择绑定的点')
    return
  }
  if (!widgetForm.title.trim()) {
    // 兜底：手快点选时也能从点表拿标题，不要求用户重复输入。
    const r = resolvePoint(widgetForm.tag)
    if (r) {
      widgetForm.title = r.bit ? `${r.point.name} · ${r.bit.name}` : r.point.name
      if (!widgetForm.unit) widgetForm.unit = r.point.unit
    }
  }
  if (!widgetForm.title.trim()) {
    toast.error('请填写组件标题')
    return
  }
  if (widgetForm.kind === 'gauge' && widgetForm.min >= widgetForm.max) {
    toast.error('仪表量程需要 max 大于 min')
    return
  }
  if (editingWidgetId.value) {
    const w = d.widgets.find((x) => x.id === editingWidgetId.value)
    if (w) {
      w.kind = widgetForm.kind
      w.title = widgetForm.title.trim()
      w.tag = widgetForm.tag
      w.unit = widgetForm.unit
      w.config = configOfForm()
      const mw = minW(w.kind)
      const mh = minH(w.kind)
      if (w.layout.w < mw) w.layout.w = Math.min(mw, d.cols - w.layout.x)
      if (w.layout.h < mh) w.layout.h = mh
      // 换成更大的组件（如 数值卡 → 仪表）后可能压到邻居：挪到第一个空位。
      if (d.widgets.some((o) => o.id !== w.id && overlaps(o.layout, w.layout))) {
        w.layout = findSlot(d, w.layout.w, w.layout.h, w.id)
      }
    }
  } else {
    const w = minW(widgetForm.kind)
    const h = minH(widgetForm.kind)
    const slot = findSlot(d, w, h)
    d.widgets.push({
      id: `w-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 5)}`,
      kind: widgetForm.kind,
      title: widgetForm.title.trim(),
      tag: widgetForm.tag,
      unit: widgetForm.unit,
      config: configOfForm(),
      layout: slot,
    })
  }
  showWidgetModal.value = false
  void saveNow()
}

function removeWidget(w: WidgetDto): void {
  const d = current.value
  if (!d) return
  const i = d.widgets.findIndex((x) => x.id === w.id)
  if (i >= 0) d.widgets.splice(i, 1)
  void saveNow()
}
</script>

<template>
  <div class="page">
    <PageHeader title="仪表盘" subtitle="网格组态：组件绑定实时 tag，布局保存在 gatewayd（UI 重装不丢）">
      <n-select
        v-if="dashStore.list.length > 1"
        v-model:value="currentId"
        size="small"
        :options="dashboardOptions"
        style="width: 180px"
      />
      <n-button size="small" :disabled="!conn.connected" @click="newDashboard">
        <template #icon><AppIcon name="plus" :size="15" /></template>
        新建看板
      </n-button>
      <n-button
        size="small"
        type="primary"
        :disabled="!current"
        @click="editing = !editing"
      >
        <template #icon><AppIcon name="layout" :size="15" /></template>
        {{ editing ? '完成布局' : '编辑布局' }}
      </n-button>
    </PageHeader>

    <div v-if="!conn.connected" class="banner warn">
      <AppIcon name="alert" :size="16" />
      <span>未连接 gatewayd，仪表盘布局不可保存。</span>
    </div>

    <div v-if="current" class="card dash-bar">
      <div class="dash-title">
        <span class="dash-glyph"><AppIcon name="layout" :size="17" /></span>
        <n-input
          v-if="editing"
          :value="nameDraft"
          size="small"
          style="width: 220px"
          @update:value="nameDraft = $event"
          @change="commitName"
          @blur="commitName"
        />
        <strong v-else>{{ current.name }}</strong>
        <span class="muted small">{{ current.widgets.length }} 个组件 · {{ current.cols }} 列</span>
      </div>
      <div class="spacer" />
      <span v-if="saveStateText" class="save-state" :class="{ error: !!saveError }" :title="saveError">
        <span v-if="dashStore.saving" class="save-spinner" />
        {{ saveStateText }}
      </span>
      <n-button v-if="editing" size="tiny" type="primary" @click="openAddWidget">
        <template #icon><AppIcon name="plus" :size="14" /></template>
        添加组件
      </n-button>
      <n-button v-if="editing" size="tiny" quaternary type="error" @click="dashboardToDelete = current">
        删除看板
      </n-button>
    </div>

    <div
      v-if="current"
      ref="canvasRef"
      class="dash-canvas"
      :class="{ editing }"
      :style="gridStyle"
    >
      <article
        v-for="w in current.widgets"
        :key="w.id"
        class="dash-widget"
        :class="[`kind-${w.kind}`, { active: draggingId === w.id }]"
        :style="widgetStyle(w)"
      >
        <header class="dw-head">
          <span v-if="editing" class="dw-grip" title="拖动位置" @pointerdown="beginDrag($event, w, 'move')">
            <AppIcon name="dots" :size="14" />
          </span>
          <span class="dw-title" :title="w.title">{{ w.title || '未命名组件' }}</span>
          <span class="dw-tag mono" :title="w.tag">{{ w.tag || '未绑定' }}</span>
          <span v-if="editing" class="dw-tools">
            <button type="button" class="icon-btn tiny" title="组件设置" @click="openWidgetSettings(w)">
              <AppIcon name="edit" :size="13" />
            </button>
            <button type="button" class="icon-btn tiny" title="删除组件" @click="removeWidget(w)">
              <AppIcon name="x" :size="14" />
            </button>
          </span>
        </header>

        <div class="dw-body">
          <!-- 数值卡 -->
          <template v-if="w.kind === 'value'">
            <div class="value-main">
              <span class="value-number" :class="qualityClass(tagChange(w.tag)?.quality)">
                {{ numberText(w) }}
              </span>
              <span v-if="w.unit" class="value-unit">{{ w.unit }}</span>
            </div>
            <div class="value-foot">
              <QualityPill compact :quality="tagChange(w.tag)?.quality" />
              <span v-if="!tagChange(w.tag)" class="muted small">等待数据</span>
            </div>
          </template>

          <!-- 开关 -->
          <template v-else-if="w.kind === 'switch'">
            <div class="switch-main">
              <button
                type="button"
                class="switch-toggle"
                :class="{ on: switchOn(w), readonly: !canWrite(w.tag) || editing }"
                :disabled="!canWrite(w.tag) || editing || writing.has(w.tag)"
                :title="canWrite(w.tag) ? '点击切换' : '只读点（布局编辑中不可操作）'"
                @click="toggleSwitch(w)"
              >
                <span class="switch-knob" />
              </button>
              <div class="switch-labels">
                <strong>{{ switchOn(w) ? cfgString(w, 'onLabel', 'ON') : cfgString(w, 'offLabel', 'OFF') }}</strong>
                <span class="muted small">{{ canWrite(w.tag) ? '可写点' : '只读 / 未绑定' }}</span>
              </div>
            </div>
            <div class="value-foot">
              <QualityPill compact :quality="tagChange(w.tag)?.quality" />
            </div>
          </template>

          <!-- 仪表 -->
          <template v-else-if="w.kind === 'gauge'">
            <div class="gauge-wrap">
              <svg class="gauge-svg" viewBox="0 0 200 108" preserveAspectRatio="xMidYMid meet">
                <path class="gauge-track" d="M20 100 A80 80 0 0 1 180 100" pathLength="100" />
                <path
                  class="gauge-fill"
                  :class="qualityClass(tagChange(w.tag)?.quality)"
                  d="M20 100 A80 80 0 0 1 180 100"
                  pathLength="100"
                  :stroke-dasharray="gaugeDash(w)"
                />
              </svg>
              <div class="gauge-readout">
                <span class="gauge-value" :class="qualityClass(tagChange(w.tag)?.quality)">{{ numberText(w) }}</span>
                <span v-if="w.unit" class="gauge-unit">{{ w.unit }}</span>
              </div>
              <div class="gauge-range">
                <span>{{ cfgNumber(w, 'min', 0) }}</span>
                <span>{{ cfgNumber(w, 'max', 100) }}</span>
              </div>
            </div>
          </template>

          <!-- 未知类型：未来插件 widget 未加载时的占位 -->
          <div v-else class="widget-unknown">
            <AppIcon name="dots" :size="18" />
            <span>未知组件类型 <code>{{ w.kind }}</code></span>
          </div>
        </div>

        <span v-if="editing" class="dw-resize" title="拖动调整大小" @pointerdown="beginDrag($event, w, 'resize')" />
      </article>

      <div v-if="!current.widgets.length" class="dash-empty">
        <div class="empty-icon"><AppIcon name="layout" :size="24" /></div>
        <h3>这个看板还没有组件</h3>
        <p class="muted">添加数值卡、开关或仪表，绑定设备点位后即可实时显示。</p>
        <n-button size="small" type="primary" @click="editing = true; openAddWidget()">添加组件</n-button>
      </div>
    </div>

    <EmptyState
      v-else-if="conn.connected && dashStore.loaded"
      icon="layout"
      title="还没有仪表盘"
      text="仪表盘布局保存在 gatewayd 的工程数据里；新建后可拖入组件并绑定点位。"
    >
      <n-button size="small" type="primary" @click="newDashboard">新建仪表盘</n-button>
    </EmptyState>

    <!-- 添加 / 编辑组件 -->
    <ModalShell
      v-if="showWidgetModal"
      :title="editingWidgetId ? '组件设置' : '添加组件'"
      subtitle="组件只绑定 tag；显示格式与量程属于组件配置"
      :width="600"
      @close="showWidgetModal = false"
    >
      <div class="form-grid">
        <div class="field">
          <label>组件类型</label>
          <n-select v-model:value="widgetForm.kind" :options="kindOptions" />
        </div>
        <div class="field">
          <label>标题</label>
          <n-input v-model:value="widgetForm.title" placeholder="例如：AB 线电压" />
        </div>
        <div class="field span-2">
          <label>绑定点位</label>
          <n-select
            :value="widgetForm.tag"
            :options="tagOptions"
            filterable
            placeholder="选择设备点位（只列出当前已添加设备的点）"
            @update:value="onTagPicked"
          />
          <p v-if="!tagOptions.length" class="hint">
            还没有可用点位：先到「设备」页添加设备，点表来自设备包。
          </p>
        </div>
        <div class="field">
          <label>单位</label>
          <n-input v-model:value="widgetForm.unit" placeholder="V / A / kWh" />
        </div>

        <template v-if="widgetForm.kind === 'value'">
          <div class="field">
            <label>小数位</label>
            <n-input-number v-model:value="widgetForm.decimals" :min="0" :max="6" style="width: 100%" />
          </div>
        </template>

        <template v-else-if="widgetForm.kind === 'gauge'">
          <div class="field">
            <label>量程下限</label>
            <n-input-number v-model:value="widgetForm.min" style="width: 100%" />
          </div>
          <div class="field">
            <label>量程上限</label>
            <n-input-number v-model:value="widgetForm.max" style="width: 100%" />
          </div>
        </template>

        <template v-else-if="widgetForm.kind === 'switch'">
          <div class="field">
            <label>闭合文案</label>
            <n-input v-model:value="widgetForm.onLabel" placeholder="ON" />
          </div>
          <div class="field">
            <label>断开文案</label>
            <n-input v-model:value="widgetForm.offLabel" placeholder="OFF" />
          </div>
        </template>
      </div>

      <div class="banner info mt-14">
        <AppIcon name="alert" :size="16" />
        <span>
          开关组件只在 <code>access=rw</code> 的寄存器/线圈点上可操作；位子 tag 恒为只读。
        </span>
      </div>

      <template #footer>
        <n-button size="small" @click="showWidgetModal = false">取消</n-button>
        <n-button size="small" type="primary" @click="submitWidget">
          {{ editingWidgetId ? '保存组件' : '添加' }}
        </n-button>
      </template>
    </ModalShell>

    <!-- 删除看板 -->
    <ModalShell
      v-if="dashboardToDelete"
      title="删除仪表盘"
      :width="440"
      @close="dashboardToDelete = null"
    >
      <p>确认删除仪表盘 <b>{{ dashboardToDelete.name }}</b>？</p>
      <p class="muted mt-8">布局会从 gatewayd 的工程数据中移除，设备与实时采集不受影响。</p>
      <template #footer>
        <n-button size="small" @click="dashboardToDelete = null">取消</n-button>
        <n-button size="small" type="error" :loading="deletingDashboard" @click="doDeleteDashboard">
          删除
        </n-button>
      </template>
    </ModalShell>
  </div>
</template>
