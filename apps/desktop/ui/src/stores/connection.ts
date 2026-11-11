import { computed } from 'vue'
import { defineStore } from 'pinia'
import { gateway } from '@/api/ws'
import { nowMs } from '@/utils/clock'
import { formatUptime } from '@/utils/format'

const STATUS_LABEL: Record<string, string> = {
  idle: '未连接',
  connecting: '连接中',
  connected: '已连接',
  reconnecting: '重连中',
  error: '连接错误',
}

export const useConnectionStore = defineStore('connection', () => {
  const state = gateway.state
  const statusLabel = computed(() => STATUS_LABEL[state.status] ?? state.status)
  const connected = computed(() => state.status === 'connected')
  const uptimeMs = computed(() =>
    state.connectedAt > 0 ? Math.max(0, nowMs.value - state.connectedAt) : 0,
  )
  const uptimeText = computed(() => formatUptime(uptimeMs.value))

  function reconnect(): Promise<void> {
    return gateway.reconnect()
  }

  function disconnect(): void {
    gateway.disconnect()
  }

  return { state, statusLabel, connected, uptimeMs, uptimeText, reconnect, disconnect }
})
