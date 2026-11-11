import { ref } from 'vue'
import { defineStore } from 'pinia'
import { gateway } from '@/api/ws'
import type { ServerMsg, UiPluginDto } from '@/api/types'

type UiPluginsMsg = Extract<ServerMsg, { type: 'uiPlugins' }>

/** 已安装 UI 插件：真值在 gatewayd（`<data-dir>/ui-plugins/`），这里只是最近一次应答。 */
export const usePluginsStore = defineStore('plugins', () => {
  const list = ref<UiPluginDto[]>([])
  const loaded = ref(false)
  const loading = ref(false)
  const busy = ref(false)

  function applyList(plugins: UiPluginDto[]): void {
    list.value = plugins
    loaded.value = true
  }

  async function refresh(force = false): Promise<UiPluginDto[]> {
    if (loaded.value && !force) return list.value
    if (loading.value) return list.value
    loading.value = true
    try {
      const r = await gateway.request<UiPluginsMsg>({ type: 'getUiPlugins' }, [
        'uiPlugins',
        'error',
      ])
      applyList(r.plugins)
      return list.value
    } finally {
      loading.value = false
    }
  }

  async function importPlugin(path: string): Promise<UiPluginDto[]> {
    busy.value = true
    try {
      const r = await gateway.request<UiPluginsMsg>({ type: 'importUiPlugin', path }, [
        'uiPlugins',
        'error',
      ])
      applyList(r.plugins)
      return r.plugins
    } finally {
      busy.value = false
    }
  }

  async function remove(id: string): Promise<UiPluginDto[]> {
    busy.value = true
    try {
      const r = await gateway.request<UiPluginsMsg>({ type: 'removeUiPlugin', id }, [
        'uiPlugins',
        'error',
      ])
      applyList(r.plugins)
      return r.plugins
    } finally {
      busy.value = false
    }
  }

  return { list, loaded, loading, busy, applyList, refresh, importPlugin, remove }
})
