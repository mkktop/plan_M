<script setup lang="ts">
import { computed } from 'vue'
import DevicePageBlock from './DevicePageBlock.vue'
import DeviceWidgetCard from './DeviceWidgetCard.vue'
import { isCompactWidgetKind, pageUsesGrid } from '@/utils/devicePages'
import type { DevicePageComponentDto } from '@/api/types'

const props = withDefaults(defineProps<{
  components: DevicePageComponentDto[]
  deviceId?: string
}>(), { deviceId: '' })

type Item = { component: DevicePageComponentDto; index: number }
type Group = { kind: 'widgets'; items: Item[] } | { kind: 'block'; item: Item }

const grid = computed(() => pageUsesGrid(props.components))
const groups = computed(() => {
  const result: Group[] = []
  props.components.forEach((component, index) => {
    const item = { component, index }
    if (!isCompactWidgetKind(component.kind)) {
      result.push({ kind: 'block', item })
      return
    }
    const last = result.at(-1)
    if (last?.kind === 'widgets') last.items.push(item)
    else result.push({ kind: 'widgets', items: [item] })
  })
  return result
})

function coordinates({ rect }: DevicePageComponentDto) {
  return rect ? {
    '--wx': rect.x, '--wy': rect.y, '--ww': rect.w, '--wh': rect.h,
  } : {}
}
</script>

<template>
  <div v-if="grid" class="ws-widget-canvas">
    <div v-for="(component, index) in components" :key="component.id ?? index"
      class="ws-canvas-item" :style="coordinates(component)">
      <DeviceWidgetCard v-if="isCompactWidgetKind(component.kind)" :device-id="deviceId" :component="component" />
      <div v-else class="ws-canvas-block">
        <div v-if="component.title" class="ws-component-head">{{ component.title }}</div>
        <DevicePageBlock :device-id="deviceId" :component="component" />
      </div>
    </div>
  </div>

  <template v-else>
    <template v-for="(group, index) in groups" :key="index">
      <div v-if="group.kind === 'widgets'" class="ws-widget-grid">
        <DeviceWidgetCard v-for="item in group.items" :key="item.component.id ?? item.index"
          :device-id="deviceId" :component="item.component" />
      </div>
      <div v-else class="ws-component">
        <div v-if="group.item.component.title" class="ws-component-head">{{ group.item.component.title }}</div>
        <DevicePageBlock :device-id="deviceId" :component="group.item.component" />
      </div>
    </template>
  </template>
</template>
