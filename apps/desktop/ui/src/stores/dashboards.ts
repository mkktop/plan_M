import { ref } from 'vue'
import { defineStore } from 'pinia'
import { gateway } from '@/api/ws'
import type { DashboardDto, ServerMsg } from '@/api/types'

type DashboardsMsg = Extract<ServerMsg, { type: 'dashboards' }>

/** 仪表盘布局：真值在 gatewayd（`<data-dir>/dashboards.json`），这里只是最近一次应答。 */
export const useDashboardsStore = defineStore('dashboards', () => {
  const list = ref<DashboardDto[]>([])
  const loaded = ref(false)
  const loading = ref(false)
  const saving = ref(false)

  function get(id: string): DashboardDto | undefined {
    return list.value.find((d) => d.id === id)
  }

  async function load(force = false): Promise<DashboardDto[]> {
    if (loaded.value && !force) return list.value
    if (loading.value) return list.value
    loading.value = true
    try {
      const r = await gateway.request<DashboardsMsg>({ type: 'getDashboards' }, [
        'dashboards',
        'error',
      ])
      list.value = r.dashboards
      loaded.value = true
      return list.value
    } finally {
      loading.value = false
    }
  }

  async function save(dashboard: DashboardDto): Promise<void> {
    saving.value = true
    try {
      const r = await gateway.request<DashboardsMsg>({ type: 'saveDashboard', dashboard }, [
        'dashboards',
        'error',
      ])
      list.value = r.dashboards
      loaded.value = true
    } finally {
      saving.value = false
    }
  }

  async function remove(id: string): Promise<void> {
    const r = await gateway.request<DashboardsMsg>({ type: 'removeDashboard', id }, [
      'dashboards',
      'error',
    ])
    list.value = r.dashboards
    loaded.value = true
  }

  return { list, loaded, loading, saving, get, load, save, remove }
})
