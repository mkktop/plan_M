import { defineStore } from 'pinia'
import { Channel, invoke } from '@tauri-apps/api/core'
import { isTauriRuntime } from '@/api/ws'

/** 与 `src-tauri/src/updates.rs` 的 UpdateCheck 同构（snake_case 保持一致）。 */
export interface UpdateCheck {
  status: 'available' | 'current' | 'unpublished'
  current_version: string
  version: string | null
  notes: string | null
  published_at: string | null
  can_install: boolean
  download_size: number | null
  installer_kind: 'windows' | null
}

interface UpdateProgress {
  downloaded: number
  total: number
}

/**
 * 应用更新状态机：idle → checking → idle / downloading → ready → installing。
 * 下载进度与结果放在 store 里，离开设置页再回来不丢状态。
 */
export const useUpdatesStore = defineStore('updates', {
  state: () => ({
    currentVersion: '',
    result: null as UpdateCheck | null,
    phase: 'idle' as 'idle' | 'checking' | 'downloading' | 'ready' | 'installing',
    downloaded: 0,
    total: 0,
    message: '',
    error: '',
  }),
  getters: {
    busy: (state) => ['checking', 'downloading', 'installing'].includes(state.phase),
    percent: (state) =>
      state.total > 0 ? Math.min(100, Math.floor((state.downloaded / state.total) * 100)) : 0,
    available: (state) =>
      state.result !== null && state.result.status === 'available' && state.result.version !== null,
  },
  actions: {
    async loadVersion(): Promise<void> {
      if (!isTauriRuntime()) return
      try {
        this.currentVersion = await invoke<string>('get_app_version')
      } catch {
        this.error = '获取当前版本号失败，请重启应用后重试'
      }
    },
    async check(): Promise<void> {
      if (this.busy) return
      if (!isTauriRuntime()) {
        this.error = '浏览器调试模式不支持应用内更新，仅桌面版可用'
        return
      }
      this.phase = 'checking'
      this.error = ''
      this.message = ''
      this.result = null
      this.downloaded = 0
      this.total = 0
      try {
        const result = await invoke<UpdateCheck>('check_for_updates')
        this.result = result
        this.currentVersion = result.current_version
        this.message =
          result.status === 'unpublished'
            ? '暂无已发布版本，请稍后再来看看'
            : result.status === 'current'
              ? '已经是最新版本'
              : `发现新版本 v${result.version}`
      } catch (e) {
        this.error = '检查更新失败：' + (e instanceof Error ? e.message : String(e))
      } finally {
        this.phase = 'idle'
      }
    },
    async download(): Promise<void> {
      if (this.busy || !this.result?.can_install || this.phase === 'ready') return
      this.phase = 'downloading'
      this.error = ''
      this.message = ''
      this.downloaded = 0
      this.total = this.result.download_size || 0
      try {
        const onProgress = new Channel<UpdateProgress>()
        onProgress.onmessage = (progress) => {
          this.downloaded = progress.downloaded
          this.total = progress.total
        }
        await invoke('download_update', { onProgress })
        this.phase = 'ready'
        this.message = '下载完成，点击「安装并重启」完成更新'
      } catch (e) {
        this.phase = 'idle'
        this.error = '下载更新失败：' + (e instanceof Error ? e.message : String(e))
      }
    },
    async install(): Promise<void> {
      if (this.phase !== 'ready') return
      this.phase = 'installing'
      this.error = ''
      try {
        await invoke('install_update')
      } catch (e) {
        this.phase = 'ready'
        this.error = '安装更新失败：' + (e instanceof Error ? e.message : String(e))
      }
    },
    async openRelease(): Promise<void> {
      try {
        await invoke('open_update_release')
      } catch (e) {
        this.error = e instanceof Error ? e.message : String(e)
      }
    },
  },
})
