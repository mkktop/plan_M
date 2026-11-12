<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { isTauriRuntime } from '@/api/ws'

// 窗口无系统边框（tauri.conf.json: decorations=false），这三个按钮是唯一的窗口控制入口。
// 浏览器里跑 UI（web-dev）时没有可控制的窗口，整块不渲染。
const win = isTauriRuntime() ? getCurrentWindow() : null
const maximized = ref(false)
let unlisten: (() => void) | null = null
let syncing = false

async function syncMaximized(): Promise<void> {
  if (!win || syncing) return
  syncing = true
  try {
    maximized.value = await win.isMaximized()
  } catch {
    // 读不到状态时保留旧图标，不影响按钮功能
  } finally {
    syncing = false
  }
}

onMounted(async () => {
  if (!win) return
  await syncMaximized()
  try {
    // 最大化/还原/系统快捷键都会触发 resize，图标跟着刷新
    unlisten = await win.onResized(() => {
      void syncMaximized()
    })
  } catch {
    // 订阅失败时按钮仍可用，只是图标可能与实际状态不同步
  }
})

onBeforeUnmount(() => unlisten?.())

function minimize(): void {
  void win?.minimize()
}

function toggleMaximize(): void {
  void win?.toggleMaximize()
}

function close(): void {
  void win?.close()
}
</script>

<template>
  <div v-if="win" class="win-ctl" @dblclick.stop>
    <button type="button" class="wc-btn" title="最小化" aria-label="最小化" @click="minimize">
      <svg viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0 5.5h10" />
      </svg>
    </button>
    <button
      type="button"
      class="wc-btn"
      :title="maximized ? '向下还原' : '最大化'"
      :aria-label="maximized ? '向下还原' : '最大化'"
      @click="toggleMaximize"
    >
      <svg v-if="maximized" viewBox="0 0 10 10" aria-hidden="true">
        <path d="M2.5 0.5H9.5V7.5" />
        <path d="M0.5 2.5H7.5V9.5H0.5Z" />
      </svg>
      <svg v-else viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0.5 0.5H9.5V9.5H0.5Z" />
      </svg>
    </button>
    <button
      type="button"
      class="wc-btn wc-close"
      title="关闭"
      aria-label="关闭"
      @click="close"
    >
      <svg viewBox="0 0 10 10" aria-hidden="true">
        <path d="M0.5 0.5L9.5 9.5M9.5 0.5L0.5 9.5" />
      </svg>
    </button>
  </div>
</template>
