<script setup lang="ts">
import { computed, nextTick, ref, useId } from 'vue'
import { NButton, NInput, NInputNumber, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import ModalShell from '@/components/ModalShell.vue'
import WidgetBindingsEditor from '@/components/device/WidgetBindingsEditor.vue'
import WidgetCatalog from '@/components/device/WidgetCatalog.vue'
import WidgetConfigEditor from '@/components/device/WidgetConfigEditor.vue'
import WidgetLayoutCanvas from '@/components/device/WidgetLayoutCanvas.vue'
import { useProjectStore } from '@/stores/project'
import { useWorkspacesStore } from '@/stores/workspaces'
import { errorMessage, useToastStore } from '@/stores/toast'
import { clonePages, normalizeComponentKind } from '@/utils/devicePages'
import { deviceWidgetOptions, widgetDefaultSize } from '@/plugins/deviceWidgets'
import { nextWidgetRect } from '@/utils/widgetGrid'
import { widgetConfigError } from '@/utils/widgetConfig'
import type { BindingDecl, BindingSpec, DevicePageDto, WidgetRect } from '@/api/types'

const props = defineProps<{
  typeId: string
  packageName: string
  pages: DevicePageDto[]
  overridden: boolean
  initialPageId?: string
  workspaceKey?: string
  scopeLabel?: string
  system?: boolean
  singlePage?: boolean
  protectedPageIds?: string[]
  inheritedLabel?: string
}>()

const emit = defineEmits<{ (e: 'close'): void; (e: 'saved'): void }>()

const project = useProjectStore()
const workspaces = useWorkspacesStore()
const toast = useToastStore()

const pkg = computed(() => project.packageById.get(props.typeId) ?? null)

interface EditorComponent {
  id?: string | null
  kind: string
  title: string
  bindText: string
  rect: WidgetRect | null
  configText: string
}

interface EditorPage {
  id: string
  title: string
  icon: string
  components: EditorComponent[]
}

function toEditor(pages: DevicePageDto[]): EditorPage[] {
  return clonePages(pages).map((p) => ({
    id: p.id,
    title: p.title,
    icon: p.icon || 'grid',
    components: p.components.map((c) => ({
      id: c.id ?? null,
      kind: normalizeComponentKind(c.kind),
      title: c.title || '',
      bindText: c.bind && Object.keys(c.bind).length ? JSON.stringify(c.bind, null, 2) : '',
      rect: c.rect ? { ...c.rect } : null,
      configText: c.config && Object.keys(c.config).length ? JSON.stringify(c.config, null, 2) : '',
    })),
  }))
}

const draft = ref<EditorPage[]>(toEditor(props.pages))
const original = JSON.stringify(draft.value)
const selectedId = ref(draft.value.find((page) => page.id === props.initialPageId)?.id ?? draft.value[0]?.id ?? '')
const saving = ref(false)
const confirmReset = ref(false)
const confirmClose = ref(false)
const dirty = computed(() => JSON.stringify(draft.value) !== original)

function requestClose(): void {
  if (saving.value) return
  if (dirty.value) confirmClose.value = true
  else emit('close')
}

const selectedPage = computed(() => draft.value.find((p) => p.id === selectedId.value) ?? null)
const editorId = useId()
const catalog = computed(() => deviceWidgetOptions(pkg.value, props.system ? { packages: project.packages } : undefined))
const byKind = computed(() => new Map(catalog.value.map((item) => [item.kind, item])))
const workspaceKey = computed(() => props.workspaceKey ?? props.typeId)
const inheritedLabel = computed(() => props.inheritedLabel ?? '包默认布局')
const scopeLabel = computed(() => props.scopeLabel ?? '同型号设备')
const bindableDevices = computed(() => project.devices.map((device) => ({
  id: device.id,
  name: device.name,
  points: project.packageById.get(device.package)?.points ?? [],
})))

function widgetSize(kind: string) {
  return byKind.value.get(kind)?.external?.size ?? widgetDefaultSize(kind, pkg.value)
}

const componentOptions = computed(() =>
  catalog.value.map((c) => ({
    label: c.label,
    value: c.kind,
  })),
)

const iconOptions = [
  'pulse',
  'layout',
  'grid',
  'settings',
  'chip',
  'activity',
  'package',
  'clock',
  'plug',
].map((i) => ({ label: i, value: i }))

function nextPageId(): string {
  const base = `page-${Date.now().toString(36)}`
  let id = base
  let n = 2
  while (draft.value.some((p) => p.id === id)) id = `${base}-${n++}`
  return id
}

function addPage(): void {
  const page: EditorPage = {
    id: nextPageId(),
    title: `新页面 ${draft.value.length + 1}`,
    icon: 'grid',
    components: [],
  }
  draft.value.push(page)
  selectedId.value = page.id
}

function removePage(page: EditorPage): void {
  if (props.protectedPageIds?.includes(page.id)) return
  if (draft.value.length <= 1) {
    toast.error('至少保留一个页面')
    return
  }
  const i = draft.value.findIndex((p) => p.id === page.id)
  if (i < 0) return
  draft.value.splice(i, 1)
  selectedId.value = draft.value[Math.max(0, i - 1)]?.id ?? draft.value[0]?.id ?? ''
}

function movePage(page: EditorPage, delta: number): void {
  const i = draft.value.findIndex((p) => p.id === page.id)
  const j = i + delta
  if (i < 0 || j < 0 || j >= draft.value.length) return
  const [p] = draft.value.splice(i, 1)
  draft.value.splice(j, 0, p)
}

function addComponent(kind: string): void {
  const page = selectedPage.value
  if (!page) return
  const rects = page.components.flatMap((component) => component.rect ? [component.rect] : [])
  const rect = pageGrid.value ? nextWidgetRect(rects, widgetSize(kind)) : null
  if (pageGrid.value && !rect) {
    toast.error('当前画布没有可用位置')
    return
  }
  page.components.push({ kind, title: '', bindText: '', rect, configText: '' })
  void revealWidget(page.components.length - 1)
}

const pageGrid = computed(
  () => selectedPage.value?.components.some((c) => c.rect) ?? false,
)

function nextRow(components: EditorComponent[]): number {
  return components.reduce((max, c) => Math.max(max, c.rect ? c.rect.y + c.rect.h : 0), 0)
}

function setPageLayout(mode: 'grid' | 'flow'): void {
  const page = selectedPage.value
  if (!page) return
  if (mode === 'flow') {
    page.components.forEach((c) => {
      c.rect = null
    })
    return
  }
  const placements: WidgetRect[] = []
  for (const c of page.components) {
    const rect = nextWidgetRect(placements, widgetSize(c.kind))
    if (!rect) {
      toast.error('组件无法放入 12 列画布，请减少组件数量')
      return
    }
    placements.push(rect)
  }
  page.components.forEach((c, index) => { c.rect = placements[index] })
}

function setKind(comp: EditorComponent, kind: string): void {
  comp.kind = kind
}

async function revealWidget(index: number): Promise<void> {
  await nextTick()
  document.getElementById(`${editorId}-widget-${index}`)?.scrollIntoView({ block: 'nearest' })
}

function moveOnCanvas(index: number, rect: WidgetRect): void {
  const component = selectedPage.value?.components[index]
  if (component) component.rect = rect
}

function ensureRect(comp: EditorComponent): WidgetRect {
  if (!comp.rect) comp.rect = { x: 0, y: nextRow(selectedPage.value?.components ?? []), w: 3, h: 2 }
  return comp.rect
}

function setRect(comp: EditorComponent, key: keyof WidgetRect, value: number | null): void {
  const rect = ensureRect(comp)
  rect[key] = Math.max(0, Math.round(value ?? 0))
}

function removeComponent(index: number): void {
  const page = selectedPage.value
  if (!page) return
  if (page.components.length <= 1) {
    toast.error('每个页面至少保留一个组件')
    return
  }
  page.components.splice(index, 1)
}

function moveComponent(index: number, delta: number): void {
  const page = selectedPage.value
  if (!page) return
  const j = index + delta
  if (j < 0 || j >= page.components.length) return
  const [c] = page.components.splice(index, 1)
  page.components.splice(j, 0, c)
}

function parseObject(text: string, label: string): Record<string, unknown> | null {
  const t = text.trim()
  if (!t) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(t)
  } catch (e) {
    throw new Error(`${label}不是合法 JSON：${errorMessage(e)}`)
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error(`${label}必须是 JSON 对象`)
  }
  return parsed as Record<string, unknown>
}

function parseConfig(text: string): Record<string, unknown> | null {
  return parseObject(text, '组件配置 config ')
}

function draftConfig(text: string): Record<string, unknown> | null {
  try {
    return parseConfig(text) ?? {}
  } catch {
    return null
  }
}

/** 绑定：槽位 → { point, bit } 或 { tag }（也接受声明数组，多点槽位）。 */
function parseBind(text: string): Record<string, BindingSpec> | null {
  const obj = parseObject(text, '组件绑定 bind ')
  if (!obj) return null
  const out: Record<string, BindingSpec> = {}
  for (const [slot, raw] of Object.entries(obj)) {
    if (!slot.trim()) throw new Error('绑定槽位名不能为空')
    if (Array.isArray(raw)) {
      if (!raw.length) throw new Error(`槽位 ${slot} 的绑定数组为空`)
      out[slot] = raw.map((decl) => parseDecl(slot, decl))
    } else {
      out[slot] = parseDecl(slot, raw)
    }
  }
  return out
}

function parseDecl(slot: string, decl: unknown): BindingDecl {
  if (!decl || typeof decl !== 'object' || Array.isArray(decl)) {
    throw new Error(`槽位 ${slot} 的绑定必须是对象`)
  }
  const d = decl as Record<string, unknown>
  const point = typeof d.point === 'string' ? d.point.trim() : ''
  const tag = typeof d.tag === 'string' ? d.tag.trim() : ''
  const bit = typeof d.bit === 'number' && Number.isInteger(d.bit) ? d.bit : undefined
  if (Boolean(point) === Boolean(tag)) {
    throw new Error(`槽位 ${slot}：point 与 tag 必须且只能填一个`)
  }
  if (bit !== undefined && !point) {
    throw new Error(`槽位 ${slot}：bit 只能与 point 组合`)
  }
  return {
    ...(point ? { point } : {}),
    ...(bit !== undefined ? { bit } : {}),
    ...(tag ? { tag } : {}),
  }
}

function validateRects(comps: EditorComponent[], pageTitle: string): void {
  const withRect = comps.filter((c) => c.rect).length
  if (withRect === 0) return
  if (withRect !== comps.length) {
    throw new Error(`页面「${pageTitle}」：网格布局要求每个组件都有 rect，或整页切回流式布局`)
  }
  const placed: { x: number; y: number; w: number; h: number; i: number }[] = []
  for (const [i, c] of comps.entries()) {
    const r = c.rect
    if (!r) continue
    if (r.w < 1 || r.h < 1) throw new Error(`页面「${pageTitle}」的组件 #${i + 1}：宽高必须大于 0`)
    if (r.x < 0 || r.y < 0 || r.x + r.w > 12) {
      throw new Error(`页面「${pageTitle}」的组件 #${i + 1}：横向超出 12 列`)
    }
    for (const p of placed) {
      if (r.x < p.x + p.w && p.x < r.x + r.w && r.y < p.y + p.h && p.y < r.y + r.h) {
        throw new Error(`页面「${pageTitle}」的组件 #${i + 1} 与组件 #${p.i + 1} 位置重叠`)
      }
    }
    placed.push({ ...r, i })
  }
}

function buildPages(): DevicePageDto[] {
  if (!draft.value.length) throw new Error('至少需要一个页面')
  return draft.value.map((p) => {
    if (!p.title.trim()) throw new Error(`页面 ${p.id} 缺少标题`)
    if (!p.components.length) throw new Error(`页面「${p.title}」至少需要一个组件`)
    validateRects(p.components, p.title)
    for (const component of p.components) {
      const invalid = widgetConfigError(byKind.value.get(component.kind)?.external?.configSchema, parseConfig(component.configText))
      if (invalid) throw new Error(`页面「${p.title}」${invalid}`)
    }
    return {
      id: p.id,
      title: p.title.trim(),
      icon: p.icon,
      components: p.components.map((c) => ({
        ...(c.id ? { id: c.id } : {}),
        kind: c.kind,
        title: c.title.trim(),
        bind: parseBind(c.bindText) ?? undefined,
        rect: c.rect ? { ...c.rect } : undefined,
        config: parseConfig(c.configText),
      })),
    }
  })
}

async function save(): Promise<void> {
  let pages: DevicePageDto[]
  try {
    pages = buildPages()
  } catch (e) {
    toast.error(errorMessage(e))
    return
  }
  saving.value = true
  try {
    await workspaces.save({ deviceType: workspaceKey.value, pages })
    toast.success(`已保存${scopeLabel.value}的页面布局`)
    emit('saved')
    emit('close')
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    saving.value = false
  }
}

async function reset(): Promise<void> {
  if (!confirmReset.value) {
    confirmReset.value = true
    return
  }
  saving.value = true
  try {
    await workspaces.remove(workspaceKey.value)
    toast.success(`已恢复${inheritedLabel.value}`)
    emit('saved')
    emit('close')
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <ModalShell
    title="编辑页面布局"
    :subtitle="`${packageName} · ${scopeLabel} · ${overridden ? '自定义布局' : inheritedLabel}`"
    :width="980"
    @close="requestClose"
  >
    <div class="page-editor">
      <aside class="page-editor-list">
        <div class="page-editor-list-head">
          <span>页面</span>
          <n-button v-if="!singlePage" size="tiny" quaternary @click="addPage">
            <template #icon><AppIcon name="plus" :size="13" /></template>
            新增
          </n-button>
        </div>

        <div
          v-for="page in draft"
          :key="page.id"
          class="page-editor-item"
          :class="{ active: page.id === selectedId }"
        >
          <button type="button" class="page-editor-trigger" :aria-current="page.id === selectedId ? 'page' : undefined"
            @click="selectedId = page.id">
            <span class="page-editor-item-icon"><AppIcon :name="page.icon || 'grid'" :size="14" /></span>
            <span class="page-editor-item-main">
              <strong>{{ page.title || '未命名页面' }}</strong>
              <small>{{ page.components.length }} 个组件</small>
            </span>
          </button>
          <span v-if="!singlePage" class="page-editor-item-tools">
            <button class="icon-btn tiny" type="button" :aria-label="`上移${page.title}`" title="上移" @click="movePage(page, -1)">↑</button>
            <button class="icon-btn tiny" type="button" :aria-label="`下移${page.title}`" title="下移" @click="movePage(page, 1)">↓</button>
            <button v-if="!protectedPageIds?.includes(page.id)" class="icon-btn tiny" type="button" aria-label="删除页面" title="删除页面" @click="removePage(page)">
              <AppIcon name="x" :size="12" />
            </button>
          </span>
        </div>
      </aside>

      <section v-if="selectedPage" class="page-editor-main">
        <div class="form-grid">
          <div class="field">
            <label>页面标题</label>
            <n-input v-model:value="selectedPage.title" placeholder="例如：设备诊断" />
          </div>
          <div class="field">
            <label>图标</label>
            <n-select v-model:value="selectedPage.icon" :options="iconOptions" />
          </div>
          <div class="field span-2">
            <label>页面 id（布局持久化用，创建后不可改）</label>
            <n-input :value="selectedPage.id" disabled class="mono" />
          </div>
        </div>

        <div class="component-editor">
          <div class="component-editor-head">
            <span>页面组件</span>
            <span class="spacer" />
            <span class="muted small">{{ pageGrid ? '网格布局（12 列）' : '流式布局' }}</span>
            <n-button size="tiny" quaternary @click="setPageLayout(pageGrid ? 'flow' : 'grid')">
              {{ pageGrid ? '切回流式' : '切到网格' }}
            </n-button>
          </div>

          <WidgetCatalog :options="catalog" @pick="addComponent" />
          <p v-if="!selectedPage.components.length" class="muted small">从组件库添加第一个组件。</p>
          <WidgetLayoutCanvas v-if="pageGrid" :key="selectedPage.id" :widgets="selectedPage.components" :options="catalog"
            @move="moveOnCanvas" @select="revealWidget" />

          <div
            v-for="(comp, index) in selectedPage.components"
            :key="index"
            :id="`${editorId}-widget-${index}`"
            class="component-row"
          >
            <div class="component-row-head">
              <n-select
                :value="comp.kind"
                :options="componentOptions"
                class="w-240"
                @update:value="(v: string) => setKind(comp, v)"
              />
              <n-input v-model:value="comp.title" placeholder="组件标题（可空）" />
              <span class="component-row-tools">
                <span class="icon-btn tiny" title="上移" @click="moveComponent(index, -1)">↑</span>
                <span class="icon-btn tiny" title="下移" @click="moveComponent(index, 1)">↓</span>
                <span class="icon-btn tiny" title="删除组件" @click="removeComponent(index)">
                  <AppIcon name="x" :size="12" />
                </span>
              </span>
            </div>
            <div v-if="pageGrid" class="component-rect-row">
              <span class="muted small">网格位置（0 基）</span>
              <label class="rect-field">
                x
                <n-input-number
                  :value="comp.rect?.x ?? 0"
                  size="tiny"
                  :min="0"
                  :max="11"
                  @update:value="(v: number | null) => setRect(comp, 'x', v)"
                />
              </label>
              <label class="rect-field">
                y
                <n-input-number
                  :value="comp.rect?.y ?? 0"
                  size="tiny"
                  :min="0"
                  @update:value="(v: number | null) => setRect(comp, 'y', v)"
                />
              </label>
              <label class="rect-field">
                w
                <n-input-number
                  :value="comp.rect?.w ?? 3"
                  size="tiny"
                  :min="1"
                  :max="12"
                  @update:value="(v: number | null) => setRect(comp, 'w', v)"
                />
              </label>
              <label class="rect-field">
                h
                <n-input-number
                  :value="comp.rect?.h ?? 2"
                  size="tiny"
                  :min="1"
                  @update:value="(v: number | null) => setRect(comp, 'h', v)"
                />
              </label>
            </div>
            <WidgetBindingsEditor v-model="comp.bindText" :kind="comp.kind" :points="pkg?.points ?? []"
              :devices="bindableDevices" :declarations="byKind.get(comp.kind)?.external?.bindings" />
            <WidgetConfigEditor v-if="draftConfig(comp.configText)" :kind="comp.kind" :config="draftConfig(comp.configText)!"
              :schema="byKind.get(comp.kind)?.external?.configSchema" :defaults="byKind.get(comp.kind)?.external?.config"
              @change="comp.configText = Object.keys($event).length ? JSON.stringify($event, null, 2) : ''" />
            <details class="widget-json">
              <summary>绑定与配置 JSON</summary>
              <div class="form-grid">
              <div class="field">
                <label>绑定 bind（槽位 → 点/位，JSON）</label>
                <n-input
                  v-model:value="comp.bindText"
                  type="textarea"
                  :autosize="{ minRows: 2, maxRows: 6 }"
                  placeholder='{ "value": { "point": "uab" } }'
                />
              </div>
              <div class="field">
                <label>配置 config（JSON）</label>
                <n-input
                  v-model:value="comp.configText"
                  type="textarea"
                  :autosize="{ minRows: 2, maxRows: 6 }"
                  placeholder="组件参数，可空"
                />
              </div>
              </div>
            </details>
          </div>
        </div>

        <div v-if="confirmReset" class="banner warn">
          <AppIcon name="alert" :size="16" />
          <span>恢复{{ inheritedLabel }}会移除{{ scopeLabel }}的布局覆盖。</span>
          <n-button size="tiny" quaternary @click="confirmReset = false">取消</n-button>
          <n-button size="tiny" type="error" :loading="saving" @click="reset">确认恢复</n-button>
        </div>
      </section>
    </div>

    <template #footer>
      <n-button
        v-if="overridden && !confirmReset"
        size="small"
        quaternary
        type="error"
        :disabled="saving"
        @click="reset"
      >
        恢复{{ inheritedLabel }}
      </n-button>
      <span class="spacer" />
      <template v-if="confirmClose">
        <span class="muted small" role="status">布局还有未保存的修改</span>
        <n-button size="small" @click="confirmClose = false">继续编辑</n-button>
        <n-button size="small" type="error" @click="emit('close')">放弃修改</n-button>
      </template>
      <n-button v-else size="small" :disabled="saving" @click="requestClose">取消</n-button>
      <n-button size="small" type="primary" :loading="saving" @click="save">保存页面</n-button>
    </template>
  </ModalShell>
</template>
