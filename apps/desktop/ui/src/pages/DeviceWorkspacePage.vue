<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NButton, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import EmptyState from '@/components/EmptyState.vue'
import DevicePagesEditor from '@/components/DevicePagesEditor.vue'
import DevicePageBlock from '@/components/device/DevicePageBlock.vue'
import DeviceWidgetCard from '@/components/device/DeviceWidgetCard.vue'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { useWorkspacesStore } from '@/stores/workspaces'
import { isCompactWidgetKind, pageUsesGrid, pagesForPackage } from '@/utils/devicePages'
import type { DevicePageComponentDto } from '@/api/types'

const route = useRoute()
const router = useRouter()
const project = useProjectStore()
const workspaces = useWorkspacesStore()
const conn = useConnectionStore()

const typeId = computed(() => String(route.params.typeId ?? ''))
const pkg = computed(() => project.packageById.get(typeId.value) ?? null)
const devices = computed(() => project.devices.filter((d) => d.package === typeId.value))
const onlineCount = computed(() => devices.value.filter((d) => d.online).length)

const saved = computed(() => workspaces.get(typeId.value))
const overridden = computed(() => Boolean(saved.value))
const pages = computed(() => pagesForPackage(pkg.value, saved.value))

const requestedPageId = computed(() =>
  typeof route.params.pageId === 'string' ? route.params.pageId : '',
)
const activePage = computed(
  () => pages.value.find((p) => p.id === requestedPageId.value) ?? pages.value[0] ?? null,
)

/** 连续的小组件合并成一行自适应网格；其余组件仍按整块纵向排列。 */
type PageBlock =
  | { type: 'widgets'; items: { comp: DevicePageComponentDto; index: number }[] }
  | { type: 'block'; comp: DevicePageComponentDto; index: number }

const pageBlocks = computed<PageBlock[]>(() => {
  const blocks: PageBlock[] = []
  activePage.value?.components.forEach((comp, index) => {
    if (isCompactWidgetKind(comp.kind)) {
      const last = blocks[blocks.length - 1]
      if (last?.type === 'widgets') last.items.push({ comp, index })
      else blocks.push({ type: 'widgets', items: [{ comp, index }] })
    } else {
      blocks.push({ type: 'block', comp, index })
    }
  })
  return blocks
})

/** 页面组件声明了 rect → 12 列绝对网格；否则走流式排列。 */
const gridMode = computed(() => {
  const page = activePage.value
  return !!page && pageUsesGrid(page.components)
})

function rectStyle(comp: DevicePageComponentDto): Record<string, string> {
  const r = comp.rect
  if (!r) return {}
  return {
    '--wx': String(r.x),
    '--wy': String(r.y),
    '--ww': String(r.w),
    '--wh': String(r.h),
  }
}

// /t/<type> 或已删除的页面 id → 规范到第一页，左侧导航才能正确高亮。
watch(
  () => [requestedPageId.value, pages.value.map((p) => p.id).join(',')] as const,
  () => {
    // 等工作区覆盖加载完再判断，否则直链/刷新自定义页面会被提前重定向到第一页。
    if (!workspaces.loaded) return
    if (!activePage.value) return
    const exists = pages.value.some((p) => p.id === requestedPageId.value)
    if (!exists) {
      void router.replace({
        path: `/t/${typeId.value}/${activePage.value.id}`,
        query: route.query,
      })
    }
  },
  { immediate: true },
)

const selectedDeviceId = ref('')
const deviceOptions = computed(() =>
  devices.value.map((d) => ({
    label: `${d.name || d.id} · #${d.slave}${d.online ? '' : '（离线）'}`,
    value: d.id,
  })),
)
const selectedDevice = computed(
  () => devices.value.find((d) => d.id === selectedDeviceId.value) ?? null,
)

watch(
  () => [route.query.device, devices.value.map((d) => d.id).join(',')] as const,
  () => {
    const q = typeof route.query.device === 'string' ? route.query.device : ''
    if (q && devices.value.some((d) => d.id === q)) {
      selectedDeviceId.value = q
      return
    }
    if (!devices.value.some((d) => d.id === selectedDeviceId.value)) {
      selectedDeviceId.value = devices.value[0]?.id ?? ''
    }
  },
  { immediate: true },
)

function onDeviceChange(value: string): void {
  selectedDeviceId.value = value
  void router.replace({
    query: value ? { ...route.query, device: value } : { ...route.query, device: undefined },
  })
}

function addDevice(): void {
  void router.push({
    path: '/devices',
    query: { add: '1', package: typeId.value },
  })
}

const editorOpen = ref(false)

onMounted(() => {
  if (conn.connected) void workspaces.load()
})
</script>

<template>
  <div class="page ws-page">
    <section v-if="pkg" class="card ws-head">
      <div class="ws-identity">
        <span class="ws-glyph"><AppIcon name="package" :size="22" /></span>
        <div class="ws-identity-main">
          <span class="ws-kicker">设备类型</span>
          <h2>{{ pkg.name }}</h2>
          <div class="ws-meta">
            <span class="mono">{{ pkg.id }}</span>
            <span>v{{ pkg.version }}</span>
            <span v-if="pkg.vendor">{{ pkg.vendor }}</span>
            <span class="ws-meta-sep" />
            <span class="online-dot" :class="{ on: onlineCount > 0 }" />
            <span>{{ onlineCount }}/{{ devices.length }} 在线</span>
          </div>
        </div>
      </div>

      <div class="ws-actions">
        <n-select
          v-if="devices.length"
          :value="selectedDeviceId"
          :options="deviceOptions"
          class="ws-device-select"
          size="small"
          placeholder="选择设备实例"
          @update:value="onDeviceChange"
        />
        <n-button size="small" @click="addDevice">
          <template #icon><AppIcon name="plus" :size="14" /></template>
          添加设备
        </n-button>
        <n-button size="small" @click="editorOpen = true">
          <template #icon><AppIcon name="settings" :size="14" /></template>
          编辑页面
        </n-button>
      </div>
    </section>

    <EmptyState
      v-if="!pkg && project.loaded"
      icon="package"
      title="设备包未安装"
      text="该设备类型对应的包可能已被移除，请到设备包页确认。"
    >
      <n-button size="small" @click="router.push('/packages')">去设备包页</n-button>
    </EmptyState>

    <template v-else-if="pkg">
      <div v-if="!devices.length" class="card ws-empty">
        <div class="empty-icon"><AppIcon name="chip" :size="24" /></div>
        <h3>这个类型还没有设备</h3>
        <p class="muted">左侧页面已由设备包定义；添加一台设备后即可查看实时数据。</p>
        <n-button size="small" type="primary" @click="addDevice">添加此型号设备</n-button>
      </div>

      <section v-if="activePage && selectedDevice" class="ws-content">
        <header class="ws-page-head">
          <div>
            <h3>{{ activePage.title }}</h3>
            <p class="muted small">
              {{ selectedDevice.name || selectedDevice.id }} · 从站 #{{ selectedDevice.slave }}
              <span v-if="selectedDevice.online" class="text-good"> · 在线</span>
              <span v-else class="text-bad"> · 离线</span>
            </p>
          </div>
          <span class="muted small mono">{{ typeId }}</span>
        </header>

        <!-- 12 列网格：组件按 rect 绝对定位，窄屏自动单列 -->
        <div v-if="gridMode" class="ws-widget-canvas">
          <div
            v-for="(comp, index) in activePage.components"
            :key="`${activePage.id}-g-${index}`"
            class="ws-canvas-item"
            :style="rectStyle(comp)"
          >
            <DeviceWidgetCard
              v-if="isCompactWidgetKind(comp.kind)"
              :device-id="selectedDevice.id"
              :component="comp"
            />
            <div v-else class="ws-canvas-block">
              <div v-if="comp.title" class="ws-component-head">{{ comp.title }}</div>
              <DevicePageBlock :device-id="selectedDevice.id" :component="comp" />
            </div>
          </div>
        </div>

        <!-- 流式：连续小组件合并为自适应网格，整块组件纵向排列 -->
        <template v-else>
          <template v-for="(block, bi) in pageBlocks" :key="`${activePage.id}-${bi}`">
            <div v-if="block.type === 'widgets'" class="ws-widget-grid">
              <DeviceWidgetCard
                v-for="item in block.items"
                :key="`${activePage.id}-${item.index}`"
                :device-id="selectedDevice.id"
                :component="item.comp"
              />
            </div>

            <div v-else class="ws-component">
              <div v-if="block.comp.title" class="ws-component-head">{{ block.comp.title }}</div>
              <DevicePageBlock :device-id="selectedDevice.id" :component="block.comp" />
            </div>
          </template>
        </template>
      </section>
    </template>

    <DevicePagesEditor
      v-if="editorOpen && pkg"
      :type-id="typeId"
      :package-name="pkg.name"
      :pages="pages"
      :overridden="overridden"
      @close="editorOpen = false"
      @saved="editorOpen = false"
    />
  </div>
</template>
