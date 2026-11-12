<script setup lang="ts">
//! 顶部设备标签栏：一个标签 = 一台设备实例，"+" 打开设备选择器。

import { computed, ref } from 'vue'
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

const activeDeviceId = computed(() =>
  typeof route.params.deviceId === 'string' ? route.params.deviceId : '',
)

/** 保持打开顺序，过滤掉已被删除的设备（prune 由 App 统一做，这里兜底）。 */
const openTabs = computed(() =>
  tabs.openIds
    .map((id) => project.devices.find((d) => d.id === id))
    .filter((d): d is NonNullable<typeof d> => Boolean(d)),
)

function openDevice(id: string): void {
  if (activeDeviceId.value === id) return
  void router.push(`/d/${id}`)
}

function closeTab(id: string): void {
  const wasActive = activeDeviceId.value === id
  const next = tabs.close(id)
  if (wasActive) void router.push(next ? `/d/${next}` : '/home')
}

function pick(id: string): void {
  tabs.open(id)
  pickerOpen.value = false
  void router.push(`/d/${id}`)
}

function addDevice(): void {
  void router.push('/devices?add=1')
}
</script>

<template>
  <div v-if="project.devices.length || openTabs.length" class="type-tabs">
    <div
      v-for="d in openTabs"
      :key="d.id"
      class="type-tab device-tab"
      :class="{ active: activeDeviceId === d.id }"
      role="button"
      tabindex="0"
      :title="`${d.name || d.id} · ${addressLabel(d.address)}`"
      @click="openDevice(d.id)"
      @keydown.enter="openDevice(d.id)"
    >
      <span class="online-dot" :class="{ on: d.online }" />
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

    <button
      type="button"
      class="type-tab-plus"
      :title="project.devices.length ? '选择设备' : '添加设备'"
      @click="project.devices.length ? (pickerOpen = true) : addDevice()"
    >
      <AppIcon name="plus" :size="15" />
    </button>

    <span class="spacer" />
    <button type="button" class="type-tabs-manage" @click="router.push('/devices')">
      <AppIcon name="chip" :size="14" />
      设备
    </button>

    <DevicePickerModal v-if="pickerOpen" @close="pickerOpen = false" @pick="pick" />
  </div>

  <div v-else-if="project.loaded" class="type-tabs empty">
    <span class="muted small">添加设备后，这里会出现设备标签页。</span>
    <button type="button" class="type-tabs-manage" @click="router.push('/devices?add=1')">
      去添加设备
    </button>
  </div>
</template>
