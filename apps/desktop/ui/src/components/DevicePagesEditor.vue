<script setup lang="ts">
import { computed, ref } from 'vue'
import { NButton, NInput, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import ModalShell from '@/components/ModalShell.vue'
import { useWorkspacesStore } from '@/stores/workspaces'
import { errorMessage, useToastStore } from '@/stores/toast'
import {
  HOST_DEVICE_COMPONENTS,
  clonePages,
} from '@/utils/devicePages'
import type { DevicePageDto } from '@/api/types'

const props = defineProps<{
  typeId: string
  packageName: string
  pages: DevicePageDto[]
  overridden: boolean
}>()

const emit = defineEmits<{ (e: 'close'): void; (e: 'saved'): void }>()

const workspaces = useWorkspacesStore()
const toast = useToastStore()

interface EditorComponent {
  kind: string
  title: string
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
      kind: c.kind,
      title: c.title || '',
      configText: c.config && Object.keys(c.config).length ? JSON.stringify(c.config, null, 2) : '',
    })),
  }))
}

const draft = ref<EditorPage[]>(toEditor(props.pages))
const selectedId = ref(draft.value[0]?.id ?? '')
const saving = ref(false)
const confirmReset = ref(false)

const selectedPage = computed(() => draft.value.find((p) => p.id === selectedId.value) ?? null)

const componentOptions = HOST_DEVICE_COMPONENTS.map((c) => ({
  label: `${c.label} · ${c.kind}`,
  value: c.kind,
}))

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
    components: [{ kind: 'overview', title: '', configText: '' }],
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
  page.components.push({ kind: 'overview', title: '', configText: '' })
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

function parseConfig(text: string): Record<string, unknown> | null {
  const t = text.trim()
  if (!t) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(t)
  } catch (e) {
    throw new Error(`组件配置不是合法 JSON：${errorMessage(e)}`)
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    throw new Error('组件配置必须是 JSON 对象')
  }
  return parsed as Record<string, unknown>
}

function buildPages(): DevicePageDto[] {
  if (!draft.value.length) throw new Error('至少需要一个页面')
  return draft.value.map((p) => {
    if (!p.title.trim()) throw new Error(`页面 ${p.id} 缺少标题`)
    if (!p.components.length) throw new Error(`页面「${p.title}」至少需要一个组件`)
    return {
      id: p.id,
      title: p.title.trim(),
      icon: p.icon,
      components: p.components.map((c) => ({
        kind: c.kind,
        title: c.title.trim(),
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
            <span class="muted small">按顺序纵向渲染，可组合多个</span>
          </div>

          <div
            v-for="(comp, index) in selectedPage.components"
            :key="index"
            class="component-row"
          >
            <div class="component-row-head">
              <n-select
                v-model:value="comp.kind"
                :options="componentOptions"
                style="width: 210px"
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
            <n-input
              v-model:value="comp.configText"
              type="textarea"
              :autosize="{ minRows: 2, maxRows: 6 }"
              placeholder="组件 config（JSON，可空）"
            />
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
