<script setup lang="ts">
//! 包编辑器（P4）：全程表单建包 —— 基本信息 / 点表 / 页面 / 预览与导出。
//!
//! 设计要点：
//! - 页面用 `schemaVersion: 2` 的 `widgets[]` 生成，绑定走相对点（point/bit）；
//! - 校验调用 gatewayd 的 `validatePackage`（与安装同一套规则），本地只做提示；
//! - 草稿自动存 localStorage，关闭弹窗不丢工作；安装走正式安装流。

import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, useId, watch } from 'vue'
import { NButton, NInput, NInputNumber, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import ModalShell from '@/components/ModalShell.vue'
import FrameFields from '@/components/FrameFields.vue'
import WidgetCatalog from '@/components/device/WidgetCatalog.vue'
import WidgetConfigEditor from '@/components/device/WidgetConfigEditor.vue'
import WidgetBindingsEditor from '@/components/device/WidgetBindingsEditor.vue'
import WidgetLayoutCanvas from '@/components/device/WidgetLayoutCanvas.vue'
import { gateway, isTauriRuntime } from '@/api/ws'
import { useProjectStore } from '@/stores/project'
import { errorMessage, useToastStore } from '@/stores/toast'
import { deviceWidgetOptions, widgetDefaultSize } from '@/plugins/deviceWidgets'
import { isCompactWidgetKind } from '@/utils/devicePages'
import { nextWidgetRect } from '@/utils/widgetGrid'
import {
  POINT_TYPES,
  areaOfRegister,
  buildManifest,
  buildPoints,
  clearDraft,
  emptyDraft,
  isBitArea,
  loadDraft,
  newPage,
  newFrame,
  newFramePoint,
  newPoint,
  newWidget,
  presetPages,
  saveDraft,
  studioBindingSlot,
  widgetBind,
  type DraftPage,
  type DraftPoint,
  type DraftWidget,
  type PackageDraft,
} from '@/utils/packageStudio'
import type { BindingSpec, SnapshotMsg, WidgetRect } from '@/api/types'
import { transportLabel } from '@/utils/transport'

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'installed', packageId: string, addDevice: boolean): void
}>()

const project = useProjectStore()
const toast = useToastStore()

const driverOptions = computed(() =>
  project.drivers.length
    ? project.drivers.map((d) => ({ label: d.name, value: d.id }))
    : [{ label: 'Modbus', value: 'modbus' }],
)
const byteOrderOptions = ['abcd', 'badc', 'cdab', 'dcba'].map((v) => ({ label: v, value: v }))

type Step = 'meta' | 'points' | 'pages' | 'preview'
const step = ref<Step>('meta')
const steps: { id: Step; label: string; hint: string }[] = [
  { id: 'meta', label: '基本信息', hint: '型号身份' },
  { id: 'points', label: '点表', hint: '寄存器与换算' },
  { id: 'pages', label: '页面', hint: '组件组合' },
  { id: 'preview', label: '预览与导出', hint: '校验 / 安装' },
]

const draft = reactive<PackageDraft>(loadDraft() ?? emptyDraft())
const driverInfo = computed(() => project.drivers.find(({ id }) => id === draft.meta.driver))
const addressFormat = computed(() => driverInfo.value?.pointAddressFormat ||
  (draft.meta.driver === 'frame' ? 'frame' : draft.meta.driver === 'modbus' ? 'register' : 'structured'))
const transportOptions = computed(() => (driverInfo.value?.transports ?? []).map((kind) => ({ label: transportLabel(kind), value: kind })))
watch(addressFormat, (format) => {
  if (format === 'frame') {
    draft.meta.frameProbe ??= newFrame()
    for (const point of draft.points) point.frame ??= newFramePoint()
  }
}, { immediate: true })
watch(() => draft.meta.driver, () => { draft.meta.transports = [] })
const selectedPageId = ref(draft.pages[0]?.id ?? '')
const selectedPage = computed(
  () => draft.pages.find((p) => p.id === selectedPageId.value) ?? draft.pages[0] ?? null,
)

const iconOptions = ['pulse', 'activity', 'grid', 'layout', 'settings', 'chip', 'package', 'clock', 'plug'].map(
  (i) => ({ label: i, value: i }),
)

const pointOptions = computed(() =>
  draft.points
    .filter((p) => p.id.trim())
    .map((p) => ({ label: `${p.name.trim() || p.id} · ${p.id}`, value: p.id })),
)
const bindingPoints = computed(() => draft.points.filter((point) => point.id.trim()).map((point) => ({
  id: point.id.trim(), name: point.name,
  bits: point.bits.filter((bit) => bit.bit !== null).map((bit) => ({ bit: bit.bit!, name: bit.name })),
})))

function updateBindings(widget: DraftWidget, text: string): void {
  widget.bindings = text ? JSON.parse(text) as Record<string, BindingSpec> : {}
}

function pointOf(id: string): DraftPoint | undefined {
  return draft.points.find((p) => p.id === id)
}

function bitOptionsOf(pointId: string): { label: string; value: number }[] {
  const p = pointOf(pointId)
  if (!p || isBitArea(p.address)) return []
  return p.bits.map((b) => ({
    label: `b${b.bit ?? 0} ${b.name || ''}`.trim(),
    value: b.bit ?? 0,
  }))
}

// —— 点表 ——

const expanded = reactive<Record<string, boolean>>({})

function addPoint(): void {
  const p = newPoint(draft.points.length)
  if (addressFormat.value === 'frame') p.frame = newFramePoint()
  const last = draft.points[draft.points.length - 1]
  const lastReg = parseInt(last?.address ?? '', 10)
  if (Number.isFinite(lastReg)) p.address = String(lastReg + 1)
  if (last?.pollGroup) p.pollGroup = last.pollGroup
  draft.points.push(p)
  expanded[p.key] = true
}

function removePoint(index: number): void {
  const [p] = draft.points.splice(index, 1)
  if (!p) return
  // 同步清掉页面上绑定该点的组件
  for (const page of draft.pages) {
    page.widgets = page.widgets.filter((w) => w.point !== p.id)
    for (const w of page.widgets) w.points = w.points.filter((x) => x !== p.id)
  }
}

function addBit(p: DraftPoint): void {
  const used = new Set(p.bits.map((b) => b.bit))
  let bit = 0
  while (used.has(bit) && bit < 16) bit += 1
  p.bits.push({ bit, name: '', on: '', off: '' })
}

function addEnum(p: DraftPoint): void {
  const used = new Set(p.enum.map((e) => e.value))
  let v = 0
  while (used.has(v)) v += 1
  p.enum.push({ value: v, label: '' })
}

function renameGroup(index: number, name: string): void {
  const old = draft.pollGroups[index]?.name ?? ''
  const g = draft.pollGroups[index]
  if (!g) return
  g.name = name
  if (old && old !== name) {
    for (const p of draft.points) if (p.pollGroup === old) p.pollGroup = name
  }
}

const groupOptions = computed(() =>
  draft.pollGroups.filter((g) => g.name.trim()).map((g) => ({ label: g.name, value: g.name })),
)

// —— 页面与组件 ——

function addPage(): void {
  const p = newPage(draft.pages.length)
  draft.pages.push(p)
  selectedPageId.value = p.id
}

function removePage(page: DraftPage): void {
  if (draft.pages.length <= 1) {
    toast.error('至少保留一个页面')
    return
  }
  const i = draft.pages.findIndex((p) => p.id === page.id)
  if (i < 0) return
  draft.pages.splice(i, 1)
  selectedPageId.value = draft.pages[Math.max(0, i - 1)]?.id ?? ''
}

const editorId = useId()
const catalog = computed(() => deviceWidgetOptions())
const externalWidgets = computed(() => new Map(catalog.value.map((item) => [item.kind, item.external])))
const widgetOptions = computed(() => catalog.value.map((widget) => ({ label: widget.label, value: widget.kind })))
const groupWidgetOptions = computed(() => widgetOptions.value.filter(({ value }) =>
  isCompactWidgetKind(value) && studioBindingSlot(value) === 'value',
))

function addWidget(kind: string): void {
  const page = selectedPage.value
  if (!page) return
  const size = widgetDefaultSize(kind)
  const rect = nextWidgetRect(page.widgets.map((widget) => widget.rect), size)
  if (!rect) {
    toast.error('当前画布没有可用位置')
    return
  }
  const w = newWidget(kind, pointOptions.value[0]?.value ?? '', size)
  w.rect = rect
  page.widgets.push(w)
  void revealWidget(page.widgets.length - 1)
}

function removeWidget(page: DraftPage, index: number): void {
  page.widgets.splice(index, 1)
}

function moveWidget(page: DraftPage, index: number, delta: number): void {
  const j = index + delta
  if (j < 0 || j >= page.widgets.length) return
  const [w] = page.widgets.splice(index, 1)
  page.widgets.splice(j, 0, w)
}

async function revealWidget(index: number): Promise<void> {
  await nextTick()
  document.getElementById(`${editorId}-widget-${index}`)?.scrollIntoView({ block: 'nearest' })
}

function moveOnCanvas(index: number, rect: WidgetRect): void {
  const widget = selectedPage.value?.widgets[index]
  if (widget) widget.rect = rect
}

function setRect(rect: WidgetRect, key: keyof WidgetRect, v: number | null): void {
  rect[key] = Math.max(0, Math.round(v ?? 0))
}

function addGroupItem(w: DraftWidget): void {
  const y = w.groupItems.reduce((m, it) => Math.max(m, it.rect.y + it.rect.h), 0)
  w.groupItems.push({
    kind: 'core:value',
    title: '',
    point: pointOptions.value[0]?.value ?? '',
    bit: null,
    rect: { x: 0, y, w: 12, h: 2 },
  })
}

function applyPresets(): void {
  const pages = presetPages(draft)
  draft.pages = pages
  selectedPageId.value = pages[0]?.id ?? ''
  toast.success('已按点表生成预设页面（可继续调整）')
}

// —— 校验（gatewayd 同一套规则）——

const validation = ref<{ errors: string[]; warnings: string[] }>({ errors: [], warnings: [] })
const validating = ref(false)
const validateError = ref('')
let validateTimer: number | undefined

async function refreshValidation(): Promise<void> {
  validating.value = true
  validateError.value = ''
  try {
    const r = await gateway.request<{ type: 'packageValidation'; errors: string[]; warnings: string[] }>(
      { type: 'validatePackage', manifest: buildManifest(draft), points: buildPoints(draft) },
      ['packageValidation', 'error'],
      10_000,
    )
    validation.value = { errors: r.errors, warnings: r.warnings }
  } catch (e) {
    validateError.value = errorMessage(e)
  } finally {
    validating.value = false
  }
}

function scheduleValidation(): void {
  if (validateTimer !== undefined) window.clearTimeout(validateTimer)
  validateTimer = window.setTimeout(() => void refreshValidation(), 600)
}

watch(
  () => JSON.stringify(draft),
  () => {
    saveDraft(draft)
    if (step.value === 'preview') scheduleValidation()
  },
  { deep: true },
)

// —— 安装 / 导出 ——

const busy = ref<'install' | 'export' | ''>('')
const exportDir = ref('')
const showJson = ref(false)

async function install(addDevice = false): Promise<void> {
  await refreshValidation()
  if (validation.value.errors.length) {
    step.value = 'preview'
    toast.error('校验未通过，先处理错误再安装')
    return
  }
  busy.value = 'install'
  try {
    const snap = await gateway.request<SnapshotMsg>(
      { type: 'installDraftPackage', manifest: buildManifest(draft), points: buildPoints(draft) },
      ['snapshot', 'error'],
      20_000,
    )
    project.applySnapshot(snap)
    toast.success(`设备包 ${draft.meta.name || draft.meta.id} v${draft.meta.version} 已安装`)
    clearDraft()
    emit('installed', draft.meta.id, addDevice)
    emit('close')
  } catch (e) {
    toast.error(`安装失败：${errorMessage(e)}`)
  } finally {
    busy.value = ''
  }
}

async function pickExportDir(): Promise<void> {
  if (!isTauriRuntime()) {
    toast.info('浏览器调试模式请手动填写导出目录（gatewayd 本机路径）')
    return
  }
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const picked = await open({ multiple: false, directory: true, title: '选择导出目录' })
    if (typeof picked === 'string' && picked) exportDir.value = picked
  } catch (e) {
    toast.error(errorMessage(e))
  }
}

async function exportPackage(): Promise<void> {
  const dir = exportDir.value.trim()
  if (!dir) {
    toast.error('请先填写导出目录')
    return
  }
  await refreshValidation()
  if (validation.value.errors.length) {
    step.value = 'preview'
    toast.error('校验未通过，先处理错误再导出')
    return
  }
  busy.value = 'export'
  try {
    await gateway.request(
      { type: 'exportPackage', manifest: buildManifest(draft), points: buildPoints(draft), dir },
      ['commandOk', 'error'],
      20_000,
    )
    toast.success(`已导出 manifest.json + points.json 到 ${dir}`)
  } catch (e) {
    toast.error(`导出失败：${errorMessage(e)}`)
  } finally {
    busy.value = ''
  }
}

function installOnly(): void {
  void install(false)
}

function installAndAdd(): void {
  void install(true)
}
function nextStep(): void {
  const i = steps.findIndex((s) => s.id === step.value)
  step.value = steps[Math.min(i + 1, steps.length - 1)]?.id ?? 'preview'
}

function resetDraft(): void {
  Object.assign(draft, emptyDraft())
  clearDraft()
  selectedPageId.value = draft.pages[0]?.id ?? ''
  toast.info('草稿已清空')
}

// —— 预览（静态，无实时数据）——

const previewPageId = ref('')
const previewPage = computed(
  () => draft.pages.find((p) => p.id === previewPageId.value) ?? selectedPage.value,
)

function previewStyle(w: DraftWidget): Record<string, string> {
  return {
    '--wx': String(w.rect.x),
    '--wy': String(w.rect.y),
    '--ww': String(w.rect.w),
    '--wh': String(w.rect.h),
  }
}

function bindText(w: DraftWidget): string {
  const slot = studioBindingSlot(w.kind)
  if (!slot) return ''
  if (slot === 'values') return w.points.length ? `${w.points.length} 个点位` : '未绑定'
  const p = pointOf(w.point)
  const label = p ? `${p.name || p.id}${w.bit !== null ? `.b${w.bit}` : ''}` : ''
  return label || '未绑定'
}

const manifestJson = computed(() => JSON.stringify(buildManifest(draft), null, 2))
const pointsJson = computed(() => JSON.stringify(buildPoints(draft), null, 2))

onMounted(() => {
  previewPageId.value = draft.pages[0]?.id ?? ''
  if (step.value === 'preview') void refreshValidation()
})

onBeforeUnmount(() => {
  if (validateTimer !== undefined) window.clearTimeout(validateTimer)
})
</script>

<template>
  <ModalShell
    title="新建设备包"
    subtitle="全程表单：点表 → 页面 → 预览；安装走与 zip 导入相同的校验/落盘流程"
    :width="1180"
    @close="emit('close')"
  >
    <div class="studio">
      <nav class="studio-steps">
        <button
          v-for="s in steps"
          :key="s.id"
          type="button"
          class="studio-step"
          :class="{ active: step === s.id }"
          @click="step = s.id; s.id === 'preview' && refreshValidation()"
        >
          <strong>{{ s.label }}</strong>
          <small>{{ s.hint }}</small>
        </button>
        <span class="spacer" />
        <span class="muted small">草稿自动保存</span>
        <n-button size="tiny" quaternary @click="resetDraft">清空草稿</n-button>
      </nav>

      <!-- 基本信息 -->
      <section v-if="step === 'meta'" class="studio-body">
        <div class="form-grid">
          <div class="field">
            <label>包 id（小写点分，如 com.acme.pm3000）</label>
            <n-input v-model:value="draft.meta.id" placeholder="com.acme.newmeter" class="mono" />
          </div>
          <div class="field">
            <label>型号名称</label>
            <n-input v-model:value="draft.meta.name" placeholder="例如：ACME 单相电表" />
          </div>
          <div class="field">
            <label>厂商</label>
            <n-input v-model:value="draft.meta.vendor" placeholder="选填" />
          </div>
          <div class="field">
            <label>版本（semver）</label>
            <n-input v-model:value="draft.meta.version" placeholder="0.1.0" class="mono" />
          </div>
          <div class="field">
            <label>驱动</label>
            <n-select v-model:value="draft.meta.driver" :options="driverOptions" />
          </div>
          <div class="field">
            <label>可用传输</label>
            <n-select v-model:value="draft.meta.transports" :options="transportOptions" multiple clearable placeholder="沿用驱动支持的全部传输" />
          </div>
          <div v-if="addressFormat === 'register'" class="field">
            <label>Modbus 封装</label>
            <n-select v-model:value="draft.meta.framing" :options="[
              { label: '按通道自动选择', value: 'auto' }, { label: 'RTU（含透传）', value: 'rtu' }, { label: 'TCP（MBAP）', value: 'tcp' },
            ]" placeholder="按通道自动选择" />
          </div>
          <div v-else-if="addressFormat === 'frame' && draft.meta.frameProbe" class="field span-2 card protocol-frame-card">
            <strong>在线探测帧</strong>
            <p class="muted small">十六进制字节用空格分隔。用 <code>{unit}</code> 代入设备地址。</p>
            <FrameFields v-model="draft.meta.frameProbe" />
          </div>
          <div v-else class="field span-2">
            <label>驱动参数（JSON）</label>
            <n-input v-model:value="draft.meta.paramsJson" type="textarea" placeholder="{}" />
          </div>
          <div class="field span-2">
            <label>描述</label>
            <n-input v-model:value="draft.meta.description" type="textarea" :autosize="{ minRows: 2, maxRows: 4 }" />
          </div>
        </div>
        <p v-if="addressFormat === 'register'" class="muted small studio-hint">
          下一步填点表：寄存器用工程记法（0xxxx 线圈 / 1xxxx 离散 / 3xxxx 输入 / 4xxxx 保持）。
        </p>
      </section>

      <!-- 点表 -->
      <section v-else-if="step === 'points'" class="studio-body">
        <div class="studio-block">
          <div class="studio-block-head">
            <strong>轮询组</strong>
            <n-button size="tiny" quaternary @click="draft.pollGroups.push({ name: `group${draft.pollGroups.length + 1}`, intervalMs: 1000 })">
              新增轮询组
            </n-button>
          </div>
          <div v-for="(g, gi) in draft.pollGroups" :key="gi" class="studio-inline">
            <n-input
              :value="g.name"
              size="small"
              placeholder="组名"
              class="w-160"
              @update:value="(v: string) => renameGroup(gi, v)"
            />
            <n-input-number
              :value="g.intervalMs"
              size="small"
              :min="50"
              :step="50"
              class="w-150"
              @update:value="(v: number | null) => (g.intervalMs = v)"
            />
            <span class="muted small">ms</span>
            <n-button
              size="tiny"
              quaternary
              type="error"
              :disabled="draft.pollGroups.length <= 1"
              @click="draft.pollGroups.splice(gi, 1)"
            >
              删除
            </n-button>
          </div>
        </div>

        <div class="studio-block">
          <div class="studio-block-head">
            <strong>点位（{{ draft.points.length }}）</strong>
            <n-button size="tiny" type="primary" @click="addPoint">
              <template #icon><AppIcon name="plus" :size="13" /></template>
              添加点
            </n-button>
          </div>

          <div v-for="(p, pi) in draft.points" :key="p.key" class="studio-point">
            <div class="studio-point-head">
              <n-input v-model:value="p.id" size="small" placeholder="id" class="mono w-130" />
              <n-input v-model:value="p.name" size="small" placeholder="显示名" class="w-180" />
              <n-input v-if="addressFormat !== 'frame'" v-model:value="p.address" size="small"
                :placeholder="addressFormat === 'register' ? '40101' : '地址对象（JSON）'" class="mono w-110" />
              <span v-if="addressFormat === 'register'" class="studio-area mono">{{ areaOfRegister(p.address) ?? '?' }}</span>
              <n-select
                v-model:value="p.type"
                size="small"
                :options="POINT_TYPES.map((t) => ({ label: t, value: t }))"
                :disabled="addressFormat === 'register' && isBitArea(p.address)"
                class="w-100"
              />
              <n-select
                v-model:value="p.byteOrder"
                size="small"
                :options="byteOrderOptions"
                :disabled="addressFormat === 'register' && isBitArea(p.address)"
                class="w-90"
              />
              <n-select
                v-model:value="p.access"
                size="small"
                :options="[{ label: '只读 r', value: 'r' }, { label: '读写 rw', value: 'rw' }]"
                :disabled="addressFormat === 'register' && ['input', 'discrete'].includes(areaOfRegister(p.address) ?? '')"
                class="w-100"
              />
              <n-select
                v-model:value="p.pollGroup"
                size="small"
                :options="groupOptions"
                class="w-120"
              />
              <span class="spacer" />
              <n-button size="tiny" quaternary @click="expanded[p.key] = !expanded[p.key]">
                {{ expanded[p.key] ? '收起' : '更多' }}
              </n-button>
              <n-button size="tiny" quaternary type="error" @click="removePoint(pi)">删除</n-button>
            </div>

            <div v-if="addressFormat === 'frame' && p.frame" class="protocol-point-frames">
              <div class="field card protocol-frame-card">
                <strong>读取帧</strong>
                <FrameFields v-model="p.frame.read" />
                <label class="field"><span>应答中数值的字节偏移（从 0 开始）</span>
                  <n-input-number v-model:value="p.frame.valueOffset" size="small" :min="0" :max="511" :precision="0" />
                </label>
              </div>
              <div v-if="p.access === 'rw'" class="field card protocol-frame-card">
                <strong>写入帧</strong>
                <FrameFields v-model="p.frame.write.frame" />
                <label class="field"><span>请求中数值的字节偏移（用 00 占位）</span>
                  <n-input-number v-model:value="p.frame.write.valueOffset" size="small" :min="0" :max="511" :precision="0" />
                </label>
              </div>
            </div>

            <div v-if="expanded[p.key]" class="studio-point-more">
              <template v-if="addressFormat !== 'register' || !isBitArea(p.address)">
                <label class="studio-mini">
                  scale
                  <n-input-number v-model:value="p.scale" size="tiny" :step="0.1" placeholder="1" />
                </label>
                <label class="studio-mini">
                  offset
                  <n-input-number v-model:value="p.offset" size="tiny" placeholder="0" />
                </label>
                <label class="studio-mini">
                  min
                  <n-input-number v-model:value="p.min" size="tiny" placeholder="不限" />
                </label>
                <label class="studio-mini">
                  max
                  <n-input-number v-model:value="p.max" size="tiny" placeholder="不限" />
                </label>
                <label class="studio-mini">
                  step
                  <n-input-number v-model:value="p.step" size="tiny" placeholder="不限" />
                </label>
              </template>
              <label class="studio-mini">
                单位
                <n-input v-model:value="p.unit" size="tiny" placeholder="V / A / kWh" class="w-110" />
              </label>

              <div v-if="!isBitArea(p.address)" class="studio-sub">
                <div class="studio-sub-head">
                  <span>状态位（可空，16 位字）</span>
                  <n-button size="tiny" quaternary @click="addBit(p)">加位</n-button>
                </div>
                <div v-for="(b, bi) in p.bits" :key="bi" class="studio-inline">
                  <n-input-number v-model:value="b.bit" size="tiny" :min="0" :max="15" class="w-72" />
                  <n-input v-model:value="b.name" size="tiny" placeholder="位名称" class="w-150" />
                  <n-input v-model:value="b.on" size="tiny" placeholder="置位文案" class="w-110" />
                  <n-input v-model:value="b.off" size="tiny" placeholder="复位文案" class="w-110" />
                  <n-button size="tiny" quaternary type="error" @click="p.bits.splice(bi, 1)">删</n-button>
                </div>
              </div>

              <div v-if="!isBitArea(p.address)" class="studio-sub">
                <div class="studio-sub-head">
                  <span>枚举写值（仅整数类型）</span>
                  <n-button size="tiny" quaternary @click="addEnum(p)">加项</n-button>
                </div>
                <div v-for="(e, ei) in p.enum" :key="ei" class="studio-inline">
                  <n-input-number v-model:value="e.value" size="tiny" class="w-90" />
                  <n-input v-model:value="e.label" size="tiny" placeholder="显示文案" class="w-150" />
                  <n-button size="tiny" quaternary type="error" @click="p.enum.splice(ei, 1)">删</n-button>
                </div>
              </div>
            </div>
          </div>
          <p v-if="!draft.points.length" class="muted small">还没有点。添加第一个点位后，页面会按点表自动生成。</p>
        </div>
      </section>

      <!-- 页面 -->
      <section v-else-if="step === 'pages'" class="studio-body studio-pages">
        <aside class="studio-page-list">
          <div class="studio-block-head">
            <strong>页面</strong>
            <n-button size="tiny" quaternary @click="addPage">新增</n-button>
          </div>
          <button
            v-for="page in draft.pages"
            :key="page.id"
            type="button"
            class="studio-page-item"
            :class="{ active: page.id === selectedPage?.id }"
            @click="selectedPageId = page.id"
          >
            <AppIcon :name="page.icon || 'grid'" :size="14" />
            <span>
              <strong>{{ page.title || page.id }}</strong>
              <small>{{ page.widgets.length }} 个组件</small>
            </span>
            <span class="icon-btn tiny" title="删除页面" @click.stop="removePage(page)">
              <AppIcon name="x" :size="12" />
            </span>
          </button>
          <n-button size="tiny" quaternary class="mt-8" @click="applyPresets">
            <template #icon><AppIcon name="grid" :size="13" /></template>
            按点表生成预设页面
          </n-button>
        </aside>

        <div v-if="selectedPage" class="studio-page-main">
          <div class="form-grid">
            <div class="field">
              <label>页面标题</label>
              <n-input v-model:value="selectedPage.title" />
            </div>
            <div class="field">
              <label>图标</label>
              <n-select v-model:value="selectedPage.icon" :options="iconOptions" />
            </div>
            <div class="field span-2">
              <label>页面 id（生成后不可改）</label>
              <n-input :value="selectedPage.id" disabled class="mono" />
            </div>
          </div>

          <WidgetCatalog :options="catalog" @pick="addWidget" />
          <WidgetLayoutCanvas v-if="selectedPage.widgets.length" :key="selectedPage.id"
            :widgets="selectedPage.widgets" :options="catalog" @move="moveOnCanvas" @select="revealWidget" />

          <div v-for="(w, wi) in selectedPage.widgets" :key="w.id"
            :id="`${editorId}-widget-${wi}`" class="studio-widget">
            <div class="studio-widget-head">
              <n-select
                v-model:value="w.kind"
                :options="widgetOptions"
                size="small"
                class="w-200"
              />
              <n-input v-model:value="w.title" size="small" placeholder="组件标题（可空）" class="w-180" />
              <label v-for="k in ['x', 'y', 'w', 'h'] as const" :key="k" class="studio-mini">
                {{ k }}
                <n-input-number
                  :value="w.rect[k]"
                  size="tiny"
                  :min="0"
                  :max="k === 'w' ? 12 : undefined"
                  class="w-72"
                  @update:value="(v: number | null) => setRect(w.rect, k, v)"
                />
              </label>
              <span class="spacer" />
              <span class="icon-btn tiny" title="上移" @click="moveWidget(selectedPage, wi, -1)">↑</span>
              <span class="icon-btn tiny" title="下移" @click="moveWidget(selectedPage, wi, 1)">↓</span>
              <span class="icon-btn tiny" title="删除" @click="removeWidget(selectedPage, wi)">
                <AppIcon name="x" :size="12" />
              </span>
            </div>

            <WidgetBindingsEditor v-if="externalWidgets.get(w.kind)"
              :model-value="JSON.stringify(widgetBind(w) ?? {})" :kind="w.kind" :points="bindingPoints"
              :declarations="externalWidgets.get(w.kind)?.bindings" @update:model-value="updateBindings(w, $event)" />
            <div v-else-if="studioBindingSlot(w.kind) === 'values'" class="widget-binding-fields">
              <label class="field">
                <span>绑定多个点位</span>
                <n-select v-model:value="w.points" :options="pointOptions" multiple filterable clearable
                  size="small" placeholder="选择点位" />
              </label>
            </div>
            <div v-else-if="studioBindingSlot(w.kind)" class="studio-widget-bind">
              <label class="studio-mini wide">
                绑定点
                <n-select
                  :value="w.point"
                  size="small"
                  :options="pointOptions"
                  filterable
                  placeholder="选择点位"
                  class="w-220"
                  @update:value="w.point = $event; w.bit = null"
                />
              </label>
              <label v-if="bitOptionsOf(w.point).length" class="studio-mini">
                位
                <n-select
                  v-model:value="w.bit"
                  size="small"
                  clearable
                  :options="bitOptionsOf(w.point)"
                  placeholder="整字"
                  class="w-140"
                />
              </label>

            </div>

            <div v-else-if="w.kind === 'core:group'" class="studio-group">
              <div class="studio-sub-head">
                <span>组合子组件</span>
                <n-button size="tiny" quaternary @click="addGroupItem(w)">加子组件</n-button>
              </div>
              <div v-for="(item, ii) in w.groupItems" :key="ii" class="studio-inline">
                <n-select
                  v-model:value="item.kind"
                  size="tiny"
                  :options="groupWidgetOptions"
                  class="w-160"
                />
                <n-input v-model:value="item.title" size="tiny" placeholder="标题" class="w-110" />
                <n-select v-model:value="item.point" size="tiny" :options="pointOptions" class="w-170" />
                <label v-for="k in ['x', 'y', 'w', 'h'] as const" :key="k" class="studio-mini">
                  {{ k }}
                  <n-input-number
                    :value="item.rect[k]"
                    size="tiny"
                    :min="0"
                    class="w-64"
                    @update:value="(v: number | null) => setRect(item.rect, k, v)"
                  />
                </label>
                <n-button size="tiny" quaternary type="error" @click="w.groupItems.splice(ii, 1)">删</n-button>
              </div>
            </div>
            <WidgetConfigEditor :kind="w.kind" :config="w.config"
              :schema="externalWidgets.get(w.kind)?.configSchema" :defaults="externalWidgets.get(w.kind)?.config" @change="w.config = $event" />
          </div>

          <p v-if="!selectedPage.widgets.length" class="muted small">
            页面还没有组件。点「按点表生成预设页面」可一键生成概览/参数/状态页，再手动微调。
          </p>
        </div>
      </section>

      <!-- 预览与导出 -->
      <section v-else class="studio-body studio-preview">
        <div class="studio-preview-main">
          <div class="studio-block-head">
            <strong>静态预览（无实时数据）</strong>
            <n-select
              v-model:value="previewPageId"
              size="small"
              :options="draft.pages.map((p) => ({ label: p.title || p.id, value: p.id }))"
              class="w-180"
            />
          </div>
          <div v-if="previewPage" class="studio-canvas">
            <div
              v-for="w in previewPage.widgets"
              :key="w.id"
              class="studio-canvas-item"
              :style="previewStyle(w)"
            >
              <strong>{{ w.title || w.kind }}</strong>
              <small class="mono">{{ w.kind }}</small>
              <span class="muted small">{{ w.kind === 'core:group' ? `${w.groupItems.length} 个子组件` : bindText(w) }}</span>
            </div>
          </div>
        </div>

        <aside class="studio-preview-side">
          <div class="studio-block">
            <div class="studio-block-head">
              <strong>校验</strong>
              <n-button size="tiny" quaternary :loading="validating" @click="refreshValidation">重新校验</n-button>
            </div>
            <p v-if="validateError" class="banner warn small">{{ validateError }}</p>
            <div v-else-if="validation.errors.length" class="studio-issues error">
              <div v-for="(e, i) in validation.errors" :key="i">{{ e }}</div>
            </div>
            <div v-else-if="validation.warnings.length" class="studio-issues warn">
              <div v-for="(w, i) in validation.warnings" :key="i">{{ w }}</div>
            </div>
            <p v-else class="muted small">
              {{ validating ? '校验中…' : '未发现问题（errors 为空即可安装）' }}
            </p>
          </div>

          <div class="studio-block">
            <div class="studio-block-head"><strong>安装</strong></div>
            <p class="muted small">
              安装到 gatewayd 数据目录（<code>{{ project.packages.length }}</code> 个包已安装），
              与 zip 导入共用校验/落盘规则。
            </p>
            <n-button size="small" type="primary" :loading="busy === 'install'" @click="installOnly">
              保存并安装
            </n-button>
            <n-button size="small" class="ml-8" :loading="busy === 'install'" @click="installAndAdd">
              安装并添加设备
            </n-button>
          </div>

          <div class="studio-block">
            <div class="studio-block-head"><strong>导出目录</strong></div>
            <div class="studio-inline">
              <n-input v-model:value="exportDir" size="small" placeholder="本机目录，如 D:\pkg\newmeter" class="mono" />
              <n-button size="tiny" quaternary @click="pickExportDir">选择</n-button>
            </div>
            <n-button size="small" :loading="busy === 'export'" @click="exportPackage">
              导出 manifest + points
            </n-button>
          </div>

          <div class="studio-block">
            <div class="studio-block-head">
              <strong>生成的 JSON</strong>
              <n-button size="tiny" quaternary @click="showJson = !showJson">
                {{ showJson ? '收起' : '展开' }}
              </n-button>
            </div>
            <pre v-if="showJson" class="studio-json mono">{{ manifestJson }}</pre>
            <pre v-if="showJson" class="studio-json mono">{{ pointsJson }}</pre>
          </div>
        </aside>
      </section>
    </div>

    <template #footer>
      <span class="muted small">
        {{ draft.pages.length }} 页 · {{ draft.points.length }} 点 ·
        {{ draft.meta.id || '未填包 id' }}
      </span>
      <span class="spacer" />
      <n-button size="small" @click="emit('close')">取消</n-button>
      <n-button
        v-if="step !== 'preview'"
        size="small"
        type="primary"
        @click="nextStep"
      >
        下一步
      </n-button>
      <n-button v-else size="small" type="primary" :loading="busy === 'install'" @click="installOnly">
        保存并安装
      </n-button>
    </template>
  </ModalShell>
</template>
