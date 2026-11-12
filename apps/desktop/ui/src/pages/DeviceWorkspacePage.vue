<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NButton } from 'naive-ui'
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

// 一个工作区 = 一台设备；页面来自该设备的设备包（按类型共享，可被用户覆盖）。
const deviceId = computed(() => String(route.params.deviceId ?? ''))
const device = computed(() => project.devices.find((d) => d.id === deviceId.value) ?? null)
const pkg = computed(() => project.packageOf(device.value) ?? null)
const channelName = computed(
  () => project.channels.find((c) => c.id === device.value?.channelId)?.name ?? '',
)

const typeId = computed(() => pkg.value?.id ?? '')
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

// 已删除的页面 id → 规范到第一页，左侧导航才能正确高亮。
watch(
  () => [requestedPageId.value, pages.value.map((p) => p.id).join(',')] as const,
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
    <section v-if="device && pkg" class="card ws-head">
      <div class="ws-identity">
        <span class="ws-glyph"><AppIcon name="chip" :size="22" /></span>
        <div class="ws-identity-main">
          <span class="ws-kicker">设备实例</span>
          <h2>{{ device.name || device.id }}</h2>
          <div class="ws-meta">
            <span class="mono">{{ device.id }}</span>
            <span>从站 #{{ device.slave }}</span>
            <span v-if="channelName">{{ channelName }}</span>
            <span class="ws-meta-sep" />
            <span class="online-dot" :class="{ on: device.online }" />
            <span>{{ device.online ? '在线' : '离线' }}</span>
            <span class="ws-meta-sep" />
            <span class="mono">{{ pkg.id }}</span>
            <span>v{{ pkg.version }}</span>
          </div>
        </div>
      </div>

      <div class="ws-actions">
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
              {{ pkg.name }} 的页面 · 按设备类型共享
              <span v-if="overridden"> · 自定义页面</span>
            </p>
          </div>
          <span class="muted small mono">{{ pkg.id }}</span>
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
              :device-id="deviceId"
              :component="comp"
            />
            <div v-else class="ws-canvas-block">
              <div v-if="comp.title" class="ws-component-head">{{ comp.title }}</div>
              <DevicePageBlock :device-id="deviceId" :component="comp" />
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
                :device-id="deviceId"
                :component="item.comp"
              />
            </div>

            <div v-else class="ws-component">
              <div v-if="block.comp.title" class="ws-component-head">{{ block.comp.title }}</div>
              <DevicePageBlock :device-id="deviceId" :component="block.comp" />
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
