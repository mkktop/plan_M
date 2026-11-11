<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { NButton } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import { mountPluginPage, pluginHost, pluginPageInfo } from '@/plugins/host'

const route = useRoute()
const host = ref<HTMLElement | null>(null)
const error = ref('')
let cleanup: (() => void) | null = null

const pluginId = computed(() => String(route.params.pluginId ?? ''))
const pageId = computed(() => String(route.params.pageId ?? ''))
const info = computed(() => pluginPageInfo(pluginId.value, pageId.value))

function unmount(): void {
  cleanup?.()
  cleanup = null
}

function mount(): void {
  unmount()
  error.value = ''
  const el = host.value
  if (!el) return
  if (!info.value) {
    error.value = `插件 ${pluginId.value} 的页面 ${pageId.value} 未安装或加载失败`
    return
  }
  try {
    cleanup = mountPluginPage(pluginId.value, pageId.value, el)
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}

// pluginHost.version：插件被安装/删除/重载后重建页面。
watch(() => [pluginId.value, pageId.value, pluginHost.version], mount)
onMounted(mount)
onBeforeUnmount(unmount)
</script>

<template>
  <div class="page">
    <PageHeader
      :title="info?.page.title ?? '插件页面'"
      :subtitle="info ? `来自 UI 插件 ${info.plugin.name}（${info.plugin.id}）` : '插件未安装或加载失败'"
    >
      <n-button size="small" disabled>
        <template #icon><AppIcon name="plugin" :size="15" /></template>
        {{ info ? `v${info.plugin.version}` : '不可用' }}
      </n-button>
    </PageHeader>

    <div v-if="error" class="banner warn">
      <AppIcon name="alert" :size="16" />
      <span>{{ error }}</span>
    </div>

    <section class="card plugin-page-card">
      <div ref="host" class="plugin-page-host" />
    </section>
  </div>
</template>
