<script setup lang="ts">
//! `core:group`：声明式子组件组合（无代码）。
//!
//! config.items = [{ kind, title?, rect, bind?, config? }]；子组件在组内 12 列网格
//! 里按 rect 排布，渲染复用 DeviceWidgetCard（可嵌套 core:group 一层）。

import { computed } from 'vue'
import DeviceWidgetCard from '@/components/device/DeviceWidgetCard.vue'
import type { DevicePageComponentDto } from '@/api/types'

const props = defineProps<{
  deviceId: string
  component: DevicePageComponentDto
}>()

const items = computed<DevicePageComponentDto[]>(() => {
  const raw = props.component.config?.items
  if (!Array.isArray(raw)) return []
  return raw
    .filter((x): x is Record<string, unknown> => Boolean(x) && typeof x === 'object')
    .map((x) => ({
      id: typeof x.id === 'string' ? x.id : null,
      kind: typeof x.kind === 'string' ? x.kind : '',
      title: typeof x.title === 'string' ? x.title : '',
      bind: (x.bind as DevicePageComponentDto['bind']) ?? undefined,
      rect: (x.rect as DevicePageComponentDto['rect']) ?? null,
      config: (x.config as DevicePageComponentDto['config']) ?? null,
    }))
})

function rectStyle(item: DevicePageComponentDto): Record<string, string> {
  const r = item.rect
  if (!r) return {}
  return {
    '--wx': String(r.x),
    '--wy': String(r.y),
    '--ww': String(r.w),
    '--wh': String(r.h),
  }
}
</script>

<template>
  <div class="dev-group-grid">
    <div v-for="(item, i) in items" :key="item.id || i" class="dev-group-item" :style="rectStyle(item)">
      <DeviceWidgetCard :device-id="deviceId" :component="item" />
    </div>
    <div v-if="!items.length" class="dev-widget-empty">
      <span>组合容器还没有子组件</span>
    </div>
  </div>
</template>
