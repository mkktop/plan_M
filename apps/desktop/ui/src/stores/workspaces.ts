import { ref } from 'vue'
import { defineStore } from 'pinia'
import { gateway } from '@/api/ws'
import type { ServerMsg, WorkspaceDto } from '@/api/types'

type WorkspacesMsg = Extract<ServerMsg, { type: 'workspaces' }>

/** 设备类型工作区覆盖：真值在 gatewayd（`<data-dir>/workspaces.json`）。 */
export const useWorkspacesStore = defineStore('workspaces', () => {
  const list = ref<WorkspaceDto[]>([])
  const loaded = ref(false)
  const loading = ref(false)
  const saving = ref(false)

  function get(deviceType: string): WorkspaceDto | undefined {
    return list.value.find((w) => w.deviceType === deviceType)
  }

  function applyList(items: WorkspaceDto[]): void {
    list.value = items
    loaded.value = true
  }

  async function load(force = false): Promise<WorkspaceDto[]> {
    if (loaded.value && !force) return list.value
    if (loading.value) return list.value
    loading.value = true
    try {
      const r = await gateway.request<WorkspacesMsg>({ type: 'getWorkspaces' }, [
        'workspaces',
        'error',
      ])
      applyList(r.workspaces)
      return list.value
    } finally {
      loading.value = false
    }
  }

  async function save(workspace: WorkspaceDto): Promise<void> {
    saving.value = true
    try {
      const r = await gateway.request<WorkspacesMsg>({ type: 'saveWorkspace', workspace }, [
        'workspaces',
        'error',
      ])
      applyList(r.workspaces)
    } finally {
      saving.value = false
    }
  }

  async function remove(deviceType: string): Promise<void> {
    saving.value = true
    try {
      const r = await gateway.request<WorkspacesMsg>({ type: 'removeWorkspace', deviceType }, [
        'workspaces',
        'error',
      ])
      applyList(r.workspaces)
    } finally {
      saving.value = false
    }
  }

  return { list, loaded, loading, saving, get, applyList, load, save, remove }
})
