<script setup lang="ts">
//! 设备页面的整块组件（v1 兼容）：概览 / 仪表盘 / 实时点表 / 参数设置。
//! 网格布局与流式布局共用同一份渲染，避免两套分支漂移。

import { computed } from 'vue'
import AppIcon from '@/components/AppIcon.vue'
import DashboardPage from '@/pages/DashboardPage.vue'
import TagsPage from '@/pages/TagsPage.vue'
import DeviceOverviewPanel from '@/components/device/DeviceOverviewPanel.vue'
import DeviceSettingsPanel from '@/components/device/DeviceSettingsPanel.vue'
import { normalizeComponentKind } from '@/utils/devicePages'
import type { DevicePageComponentDto } from '@/api/types'

const props = defineProps<{
  deviceId: string
  component: DevicePageComponentDto
}>()

const kind = computed(() => normalizeComponentKind(props.component.kind))
</script>

<template>
  <DeviceOverviewPanel v-if="kind === 'core:overview'" :device-id="deviceId" />
  <DashboardPage
    v-else-if="kind === 'core:dashboard'"
    embedded
    scope-kind="device"
    :scope-id="deviceId"
  />
  <TagsPage v-else-if="kind === 'core:tags'" embedded :device-id="deviceId" />
  <DeviceSettingsPanel v-else-if="kind === 'core:settings'" :device-id="deviceId" />
  <div v-else class="banner warn">
    <AppIcon name="alert" :size="16" />
    <span>未知组件 <code>{{ component.kind }}</code>，请升级宿主或修改设备包。</span>
  </div>
</template>
