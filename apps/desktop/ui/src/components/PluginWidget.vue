<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import AppIcon from '@/components/AppIcon.vue'
import { mountPluginWidget, pluginHost } from '@/plugins/host'
import type { WidgetDto } from '@/api/types'

const props = defineProps<{ widget: WidgetDto; editing: boolean }>()

const host = ref<HTMLElement | null>(null)
const error = ref('')
let cleanup: (() => void) | null = null

function unmount(): void {
  cleanup?.()
  cleanup = null
}

function mount(): void {
  unmount()
  error.value = ''
  const el = host.value
  if (!el) return
  try {
    cleanup = mountPluginWidget(props.widget.kind, el, props.widget, props.editing)
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}

onMounted(mount)
onBeforeUnmount(unmount)

// 插件加载/删除、绑定点或 config 变化、进入/退出布局编辑都需要重建实例。
watch(
  () => [
    props.widget.kind,
    props.widget.tag,
    JSON.stringify(props.widget.config ?? null),
    props.editing,
    pluginHost.version,
  ],
  mount,
)
</script>

<template>
  <div class="plugin-widget-host">
    <div ref="host" class="plugin-widget-slot" />
    <div v-if="error" class="widget-unknown plugin-widget-error">
      <AppIcon name="alert" :size="17" />
      <span>插件组件不可用</span>
      <code>{{ error }}</code>
    </div>
  </div>
</template>
