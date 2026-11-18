<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NButton, NDropdown } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import EmptyState from '@/components/EmptyState.vue'
import DevicePagesEditor from '@/components/DevicePagesEditor.vue'
import WorkspaceCanvas from '@/components/device/WorkspaceCanvas.vue'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { useWorkspacesStore } from '@/stores/workspaces'
import { normalizeComponentKind, pagesForPackage } from '@/utils/devicePages'
import { instanceWorkspaceKey } from '@/utils/systemPages'
import { addressLabel } from '@/utils/transport'

const route = useRoute()
const router = useRouter()
const project = useProjectStore()
const workspaces = useWorkspacesStore()
const conn = useConnectionStore()

// 一个工作区 = 一台设备；页面来自该设备的设备包（按类型共享，可被用户覆盖）。
const deviceId = computed(() => String(route.params.deviceId ?? ''))
const device = computed(() => project.devices.find((d) => d.id === deviceId.value) ?? null)
const pkg = computed(() => project.packageOf(device.value) ?? null)
const channelName = computed(
  () => project.channels.find((c) => c.id === device.value?.channelId)?.name ?? '',
)

const typeId = computed(() => pkg.value?.id ?? '')
const modelLayout = computed(() => workspaces.get(typeId.value))
const instanceKey = computed(() => instanceWorkspaceKey(deviceId.value))
const instanceLayout = computed(() => workspaces.get(instanceKey.value))
const modelPages = computed(() => pagesForPackage(pkg.value, modelLayout.value))
const pages = computed(() => pagesForPackage(pkg.value, instanceLayout.value ?? modelLayout.value))

const requestedPageId = computed(() =>
  typeof route.params.pageId === 'string' ? route.params.pageId : '',
)
const activePage = computed(
  () => pages.value.find((p) => p.id === requestedPageId.value) ?? pages.value[0] ?? null,
)
const pointTablePage = computed(() => pages.value.find((page) =>
  page.components.some((component) => normalizeComponentKind(component.kind) === 'core:tags'),
))

// 已删除的页面 id → 规范到第一页，左侧导航才能正确高亮。
watch(
  () => [deviceId.value, workspaces.loaded, requestedPageId.value, pages.value.map((p) => p.id).join(',')] as const,
  () => {
    if (!workspaces.loaded) return
    if (!activePage.value) return
    const exists = pages.value.some((p) => p.id === requestedPageId.value)
    if (!exists && deviceId.value) {
      void router.replace(`/d/${deviceId.value}/${activePage.value.id}`)
    }
  },
  { immediate: true },
)

const editorScope = ref<'device' | 'model' | null>(null)
const editingInstance = computed(() => editorScope.value === 'device')
const editChoices = [
  { key: 'device', label: '仅编辑当前设备' },
  { key: 'model', label: '编辑同型号共享布局' },
]

function editLayout(key: string): void {
  editorScope.value = key === 'model' ? 'model' : 'device'
}

watch(deviceId, () => { editorScope.value = null })

onMounted(() => {
  if (conn.connected) void workspaces.load()
})
</script>

<template>
  <div class="page ws-page">
    <section v-if="device && pkg" class="card ws-head">
      <div class="ws-identity">
        <span class="ws-glyph"><AppIcon name="chip" :size="22" /></span>
        <div class="ws-identity-main">
          <h2>{{ device.name || device.id }}</h2>
          <div class="ws-meta">
            <span class="mono">{{ device.id }}</span>
            <span>{{ addressLabel(device.address) }}</span>
            <span v-if="channelName">{{ channelName }}</span>
            <span class="ws-meta-sep" />
            <span class="state-pill" :class="project.live && device.online ? 'ok' : 'idle'">
              {{ project.live ? (device.online ? '在线' : '离线') : '状态待确认' }}
            </span>
            <span class="ws-meta-sep" />
            <span>{{ pkg.name }} · v{{ pkg.version }}</span>
          </div>
        </div>
      </div>

      <div class="ws-actions">
        <n-button
          v-if="!pointTablePage || pointTablePage.id !== activePage?.id"
          size="small"
          @click="router.push(pointTablePage ? `/d/${deviceId}/${pointTablePage.id}` :
            { path: '/diagnostics', query: { tab: 'data', device: deviceId } })"
        >
          <template #icon><AppIcon name="activity" :size="14" /></template>
          {{ pointTablePage ? '实时点表' : '跨设备数据浏览' }}
        </n-button>
        <n-dropdown trigger="click" :options="editChoices" @select="editLayout">
          <n-button size="small" :disabled="!conn.connected">
            <template #icon><AppIcon name="edit" :size="14" /></template>
            编辑页面
          </n-button>
        </n-dropdown>
      </div>
    </section>

    <EmptyState
      v-if="!device && project.loaded"
      icon="chip"
      title="设备不存在"
      text="该设备可能已被删除，请到设备页确认，或从顶部「+」重新选择。"
    >
      <n-button size="small" @click="router.push('/devices')">去设备页</n-button>
    </EmptyState>

    <EmptyState
      v-else-if="device && !pkg && project.loaded"
      icon="package"
      title="设备包未安装"
      text="该设备对应的设备包已被移除，请先重新导入。"
    >
      <n-button size="small" @click="router.push('/packages')">去设备包页</n-button>
    </EmptyState>

    <template v-else-if="device && pkg">
      <section v-if="activePage" class="ws-content">
        <header class="ws-page-head">
          <div>
            <h3>{{ activePage.title }}</h3>
            <p class="muted small">
              {{ instanceLayout ? '当前设备布局' : modelLayout ? '同型号共享布局' : '设备包默认布局' }}
            </p>
          </div>
        </header>

        <WorkspaceCanvas :key="activePage.id" :device-id="deviceId" :components="activePage.components" />
      </section>
    </template>

    <DevicePagesEditor
      v-if="editorScope && pkg"
      :type-id="typeId"
      :package-name="editingInstance ? (device?.name || deviceId) : pkg.name"
      :workspace-key="editingInstance ? instanceKey : typeId"
      :scope-label="editingInstance ? '当前设备' : '同型号设备'"
      :inherited-label="editingInstance && modelLayout ? '同型号共享布局' : '包默认布局'"
      :pages="editingInstance ? pages : modelPages"
      :overridden="Boolean(editingInstance ? instanceLayout : modelLayout)"
      :initial-page-id="activePage?.id"
      @close="editorScope = null"
    />
  </div>
</template>
