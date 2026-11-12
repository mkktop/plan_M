<script setup lang="ts">
import { computed, ref } from 'vue'
import { NButton, NInput, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import ModalShell from '@/components/ModalShell.vue'
import { useProjectStore } from '@/stores/project'
import { useWorkspacesStore } from '@/stores/workspaces'
import { errorMessage, useToastStore } from '@/stores/toast'
import { clonePages, normalizeComponentKind } from '@/utils/devicePages'
import { deviceWidgetOptions, widgetDefaultSize } from '@/plugins/deviceWidgets'
import type { BindingDecl, BindingSpec, DevicePageDto, WidgetRect } from '@/api/types'

const props = defineProps<{
  typeId: string
  packageName: string
  pages: DevicePageDto[]
  overridden: boolean
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
const selectedId = ref(draft.value[0]?.id ?? '')
const saving = ref(false)
const confirmReset = ref(false)

const selectedPage = computed(() => draft.value.find((p) => p.id === selectedId.value) ?? null)

const componentOptions = computed(() =>
  deviceWidgetOptions(pkg.value).map((c) => ({
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
    components: [{ kind: 'core:overview', title: '', bindText: '', rect: null, configText: '' }],
  }
  draft.value.push(page)
  selectedId.value = page.id
}

function removePage(page: EditorPage): void {
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

function addComponent(): void {
  const page = selectedPage.value
  if (!page) return
  const grid = pageGrid.value
  const kind = grid ? 'core:value' : 'core:overview'
  const size = widgetDefaultSize(kind, pkg.value)
  const rect = grid ? { x: 0, y: nextRow(page.components), ...size } : null
  page.components.push({ kind, title: '', bindText: '', rect, configText: '' })
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
  let y = 0
  page.components.forEach((c) => {
    const size = widgetDefaultSize(c.kind, pkg.value)
    c.rect = { x: 0, y, ...size }
    y += size.h
  })
}

function setKind(comp: EditorComponent, kind: string): void {
  comp.kind = kind
  if (comp.rect) {
    const size = widgetDefaultSize(kind, pkg.value)
    comp.rect.w = size.w
    comp.rect.h = size.h
  }
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
    await workspaces.save({ deviceType: props.typeId, pages })
    toast.success('设备页面已保存')
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
    await workspaces.remove(props.typeId)
    toast.success('已恢复设备包默认页面')
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
    title="编辑设备页面"
    :subtitle="`${packageName} · 页面按设备类型共享${overridden ? ' · 当前使用自定义页面' : ' · 当前使用包默认页面'}`"
    :width="980"
    @close="emit('close')"
  >
    <div class="page-editor">
      <aside class="page-editor-list">
        <div class="page-editor-list-head">
          <span>页面</span>
          <n-button size="tiny" quaternary @click="addPage">
            <template #icon><AppIcon name="plus" :size="13" /></template>
            新增
          </n-button>
        </div>

        <button
          v-for="page in draft"
          :key="page.id"
          type="button"
          class="page-editor-item"
          :class="{ active: page.id === selectedId }"
          @click="selectedId = page.id"
        >
          <span class="page-editor-item-icon"><AppIcon :name="page.icon || 'grid'" :size="14" /></span>
          <span class="page-editor-item-main">
            <strong>{{ page.title || '未命名页面' }}</strong>
            <small>{{ page.components.length }} 个组件</small>
          </span>
          <span class="page-editor-item-tools">
            <span class="icon-btn tiny" title="上移" @click.stop="movePage(page, -1)">↑</span>
            <span class="icon-btn tiny" title="下移" @click.stop="movePage(page, 1)">↓</span>
            <span class="icon-btn tiny" title="删除页面" @click.stop="removePage(page)">
              <AppIcon name="x" :size="12" />
            </span>
          </span>
        </button>
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

          <div
            v-for="(comp, index) in selectedPage.components"
            :key="index"
            class="component-row"
          >
            <div class="component-row-head">
              <n-select
                :value="comp.kind"
                :options="componentOptions"
                style="width: 240px"
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
          </div>

          <n-button size="small" quaternary @click="addComponent">
            <template #icon><AppIcon name="plus" :size="14" /></template>
            添加组件
          </n-button>
        </div>

        <div v-if="confirmReset" class="banner warn">
          <AppIcon name="alert" :size="16" />
          <span>恢复默认会丢弃当前类型的所有自定义页面，确认继续？</span>
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
        恢复包默认页面
      </n-button>
      <span class="spacer" />
      <n-button size="small" @click="emit('close')">取消</n-button>
      <n-button size="small" type="primary" :loading="saving" @click="save">保存页面</n-button>
    </template>
  </ModalShell>
</template>
