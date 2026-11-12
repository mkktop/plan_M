<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { NButton } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { errorMessage, useToastStore } from '@/stores/toast'

// 本机串口枚举：新建总线的入口（选口即可预填端口/ID/名称）。
const emit = defineEmits<{ use: [port: string] }>()

const project = useProjectStore()
const conn = useConnectionStore()
const toast = useToastStore()
const loading = ref(false)

async function refresh(): Promise<void> {
  if (!conn.connected) {
    toast.error('先连接 gatewayd 才能枚举串口')
    return
  }
  loading.value = true
  try {
    await project.loadSerialPorts()
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  if (conn.connected) void refresh()
})

// 先打开页面、后连上 gatewayd 时自动枚举一次，避免用户再点刷新。
watch(
  () => conn.connected,
  (on) => {
    if (on) void refresh()
  },
)
</script>

<template>
  <section class="card ports-card">
    <div class="card-head">
      <div class="card-head-main">
        <h3>本机串口</h3>
        <span class="muted">插上 USB-485 后点「用此口建通道」；按硬件 ID 匹配可防止 COM 号漂移</span>
      </div>
      <n-button size="tiny" :loading="loading" @click="refresh">
        <template #icon><AppIcon name="refresh" :size="14" /></template>
        刷新串口
      </n-button>
    </div>
    <div v-if="project.serialPorts.length" class="port-list">
      <div v-for="p in project.serialPorts" :key="p.portName" class="port-row">
        <div class="port-name mono">{{ p.portName }}</div>
        <div class="port-desc">
          <span>{{ p.description || '串行设备' }}</span>
          <code v-if="p.hwId" class="hw-id">{{ p.hwId }}</code>
        </div>
        <n-button size="tiny" @click="emit('use', p.portName)">用此口建通道</n-button>
      </div>
    </div>
    <div v-else class="port-empty muted">
      {{
        !conn.connected
          ? '未连接 gatewayd，连接后才可枚举串口'
          : loading
            ? '正在枚举串口…'
            : '未发现串口（检查 USB-485 是否插好，或点击刷新）'
      }}
    </div>
  </section>
</template>
