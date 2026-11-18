<script setup lang="ts">
//! 顶部设备标签栏：一个标签 = 一台设备实例。
//! 溢出时横向滚动（两端渐隐 + 边缘按钮），支持拖拽排序与中键关闭；"+" 打开设备选择器。

import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppIcon from '@/components/AppIcon.vue'
import DevicePickerModal from '@/components/device/DevicePickerModal.vue'
import { useDeviceTabsStore } from '@/stores/deviceTabs'
import { useProjectStore } from '@/stores/project'
import { addressLabel } from '@/utils/transport'

const route = useRoute()
const router = useRouter()
const project = useProjectStore()
const tabs = useDeviceTabsStore()

const pickerOpen = ref(false)

// —— 溢出滚动 ——
const scroller = ref<HTMLElement | null>(null)
const canLeft = ref(false)
const canRight = ref(false)

function syncScroll(): void {
  const el = scroller.value
  if (!el) return
  canLeft.value = el.scrollLeft > 1
  canRight.value = el.scrollLeft + el.clientWidth < el.scrollWidth - 1
}

function scrollEdge(dir: -1 | 1): void {
  scroller.value?.scrollBy({ left: dir * 220, behavior: 'smooth' })
}

function onResize(): void {
  syncScroll()
}

onMounted(() => {
  syncScroll()
  window.addEventListener('resize', onResize)
})

onBeforeUnmount(() => {
  window.removeEventListener('resize', onResize)
})

const activeDeviceId = computed(() =>
  typeof route.params.deviceId === 'string' ? route.params.deviceId : '',
)

/** 保持打开顺序，过滤掉已被删除的设备（prune 由 App 统一做，这里兜底）。 */
const openTabs = computed(() =>
  tabs.openIds
    .map((id) => project.devices.find((d) => d.id === id))
    .filter((d): d is NonNullable<typeof d> => Boolean(d)),
)

// 标签数量变化后重算渐隐；激活标签变化后把它滚入视野。
watch(openTabs, () => nextTick(syncScroll))
watch(activeDeviceId, () => {
  nextTick(() => {
    scroller.value
      ?.querySelector('.type-tab.active')
      ?.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior: 'smooth' })
  })
})

// —— 拖拽排序（HTML5 DnD） ——
const dragFrom = ref(-1)
const dropAt = ref(-1) // 插入位置：插到该序号标签之前

function onDragStart(e: DragEvent, index: number): void {
  dragFrom.value = index
  dropAt.value = index
  if (e.dataTransfer) {
    e.dataTransfer.effectAllowed = 'move'
    e.dataTransfer.setData('text/plain', String(index))
  }
}

function onDragOver(e: DragEvent, index: number): void {
  if (dragFrom.value < 0) return
  e.preventDefault()
  if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
  dropAt.value = e.clientX - rect.left > rect.width / 2 ? index + 1 : index
}

function onDrop(e: DragEvent): void {
  e.preventDefault()
  const from = dragFrom.value
  const insertAt = dropAt.value
  dragFrom.value = -1
  dropAt.value = -1
  if (from < 0 || insertAt < 0) return
  const target = insertAt > from ? insertAt - 1 : insertAt
  tabs.reorder(from, target)
  nextTick(syncScroll)
}

function onDragEnd(): void {
  dragFrom.value = -1
  dropAt.value = -1
}

function dropClass(index: number): Record<string, boolean> {
  if (dragFrom.value < 0) return {}
  if (dropAt.value === index) return { 'drop-before': true }
  if (dropAt.value === index + 1) return { 'drop-after': true }
  return {}
}

function openDevice(id: string): void {
  if (activeDeviceId.value === id) return
  void router.push(`/d/${id}`)
}

function closeTab(id: string): void {
  const wasActive = activeDeviceId.value === id
  const next = tabs.close(id)
  if (wasActive) void router.push(next ? `/d/${next}` : '/home')
}

/** 中键关闭（click 只对主键触发，走 auxclick）。 */
function onAuxClick(e: MouseEvent, id: string): void {
  if (e.button === 1) {
    e.preventDefault()
    closeTab(id)
  }
}

function pick(id: string): void {
  tabs.open(id)
  pickerOpen.value = false
  void router.push(`/d/${id}`)
}

</script>

<template>
  <div v-if="openTabs.length" class="type-tabs" aria-label="设备标签">
    <button
      v-if="canLeft"
      type="button"
      class="type-tabs-edge left"
      title="向左滚动"
      @click="scrollEdge(-1)"
    >
      <AppIcon name="chevronDown" :size="13" />
    </button>

    <div
      ref="scroller"
      class="type-tabs-scroll"
      :class="{ 'can-left': canLeft, 'can-right': canRight }"
      @scroll="syncScroll"
      @dragover.prevent
      @drop="onDrop"
    >
      <div
        v-for="(d, i) in openTabs"
        :key="d.id"
        class="type-tab device-tab"
        :class="[{ active: activeDeviceId === d.id, dragging: dragFrom === i }, dropClass(i)]"
        draggable="true"
        role="button"
        tabindex="0"
        :title="`${d.name || d.id} · ${addressLabel(d.address)}`"
        @click="openDevice(d.id)"
        @keydown.enter="openDevice(d.id)"
        @auxclick="onAuxClick($event, d.id)"
        @dragstart="onDragStart($event, i)"
        @dragover="onDragOver($event, i)"
        @dragend="onDragEnd"
      >
        <span class="online-dot" :class="{ on: project.live && d.online }"
          :title="project.live ? (d.online ? '在线' : '离线') : '状态待确认'" />
        <span class="type-tab-name">{{ d.name || d.id }}</span>
        <span class="type-tab-sub mono">{{ addressLabel(d.address) }}</span>
        <button
          type="button"
          class="type-tab-close"
          title="关闭标签页"
          @click.stop="closeTab(d.id)"
        >
          <AppIcon name="x" :size="11" />
        </button>
      </div>
    </div>

    <button
      v-if="canRight"
      type="button"
      class="type-tabs-edge right"
      title="向右滚动"
      @click="scrollEdge(1)"
    >
      <AppIcon name="chevronDown" :size="13" />
    </button>

    <button
      type="button"
      class="type-tab-plus"
      title="打开设备"
      aria-label="打开设备"
      @click="pickerOpen = true"
    >
      <AppIcon name="plus" :size="15" />
    </button>

    <button type="button" class="type-tabs-manage" @click="router.push('/devices')">
      <AppIcon name="chip" :size="14" />
      设备
    </button>

    <DevicePickerModal v-if="pickerOpen" @close="pickerOpen = false" @pick="pick" />
  </div>

</template>
