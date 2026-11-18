<script setup lang="ts">
//! 设备页面的整块组件（v1 兼容）：概览 / 仪表盘 / 实时点表 / 参数设置。
//! 网格布局与流式布局共用同一份渲染，避免两套分支漂移。

import { computed, defineAsyncComponent } from 'vue'
import AppIcon from '@/components/AppIcon.vue'
import DashboardPage from '@/pages/DashboardPage.vue'
import TagsPage from '@/pages/TagsPage.vue'
import DeviceOverviewPanel from '@/components/device/DeviceOverviewPanel.vue'
import DeviceSettingsPanel from '@/components/device/DeviceSettingsPanel.vue'
import { normalizeComponentKind } from '@/utils/devicePages'
import { systemModules } from '@/utils/systemPages'
import type { DevicePageComponentDto } from '@/api/types'

const props = defineProps<{
  deviceId: string
  component: DevicePageComponentDto
}>()

const kind = computed(() => normalizeComponentKind(props.component.kind))
const systemBlocks = new Map(systemModules.map((module) => [
  `system:${module.id}`, defineAsyncComponent(module.load),
]))
const pluginPage = defineAsyncComponent(() => import('@/pages/PluginPage.vue'))
const systemBlock = computed(() => systemBlocks.get(kind.value))
const pluginPageIds = computed(() => kind.value.startsWith('system-plugin:') ? kind.value.split(':').slice(1) : [])
</script>

<template>
  <component :is="systemBlock" v-if="systemBlock" />
  <component :is="pluginPage" v-else-if="pluginPageIds.length === 2" :plugin-id="pluginPageIds[0]" :page-id="pluginPageIds[1]" />
  <DeviceOverviewPanel v-else-if="kind === 'core:overview'" :device-id="deviceId" />
  <DashboardPage
    v-else-if="kind === 'core:dashboard'"
    embedded
    :scope-kind="deviceId ? 'device' : 'global'"
    :scope-id="deviceId || undefined"
  />
  <TagsPage v-else-if="kind === 'core:tags'" embedded :device-id="deviceId || undefined" />
  <DeviceSettingsPanel v-else-if="kind === 'core:settings'" :device-id="deviceId" />
  <div v-else class="banner warn">
    <AppIcon name="alert" :size="16" />
    <span>未知组件 <code>{{ component.kind }}</code>，请升级宿主或修改设备包。</span>
  </div>
</template>
