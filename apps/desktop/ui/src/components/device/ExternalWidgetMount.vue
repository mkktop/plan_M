<script setup lang="ts">
//! 外部 ESM 组件的 Vue 壳：负责挂载/重挂/卸载，渲染失败时显示错误卡片。
//!
//! 真正的协议实现在 `plugins/widgetHost.ts`；这里只管生命周期与错误隔离。

import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import AppIcon from '@/components/AppIcon.vue'
import { mountExternalWidget, type WidgetInstance } from '@/plugins/widgetHost'
import type { ExternalWidgetDescriptor } from '@/plugins/deviceWidgets'
import type { DevicePageComponentDto, PackageDto } from '@/api/types'

const props = defineProps<{
  descriptor: ExternalWidgetDescriptor
  component: DevicePageComponentDto
  deviceId: string
  pkg?: PackageDto | null
  mode?: 'view' | 'edit'
}>()

const el = ref<HTMLElement | null>(null)
const error = ref('')
let instance: WidgetInstance | null = null
let seq = 0

async function mount(): Promise<void> {
  const my = ++seq
  instance?.dispose()
  instance = null
  error.value = ''
  const target = el.value
  if (!target) return
  try {
    const inst = await mountExternalWidget({
      el: target,
      descriptor: props.descriptor,
      component: props.component,
      deviceId: props.deviceId,
      pkg: props.pkg,
      mode: props.mode ?? 'view',
    })
    if (my !== seq) {
      inst.dispose()
      return
    }
    instance = inst
  } catch (e) {
    if (my !== seq) return
    error.value = e instanceof Error ? e.message : String(e)
  }
}

function remount(): void {
  if (instance?.update()) return
  void mount()
}

onMounted(() => void mount())

onBeforeUnmount(() => {
  seq += 1
  instance?.dispose()
  instance = null
})

watch(
  () => [
    props.descriptor.moduleUrl,
    props.descriptor.kind,
    props.deviceId,
    props.mode,
    JSON.stringify(props.component.bind ?? null),
    JSON.stringify(props.component.config ?? null),
  ],
  remount,
)
</script>

<template>
  <div class="ext-widget-body">
    <div ref="el" class="ext-widget-slot" />
    <div v-if="error" class="dev-widget-empty ext-widget-error">
      <AppIcon name="alert" :size="15" />
      <span>组件加载失败</span>
      <code>{{ error }}</code>
    </div>
  </div>
</template>
