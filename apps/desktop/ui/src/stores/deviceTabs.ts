import { ref } from 'vue'
import { defineStore } from 'pinia'

//! 顶部设备标签：每个标签 = 一台设备实例。
//! 只记录「打开了哪些设备」；激活标签由当前路由决定，不在这里存。

const STORAGE_KEY = 'hmi.deviceTabs'

function load(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '[]') as unknown
    return Array.isArray(raw) ? raw.filter((x): x is string => typeof x === 'string' && !!x) : []
  } catch {
    return []
  }
}

export const useDeviceTabsStore = defineStore('deviceTabs', () => {
  const openIds = ref<string[]>(load())

  function persist(): void {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(openIds.value))
    } catch {
      /* 隐私模式下忽略 */
    }
  }

  /** 打开（或聚焦）一台设备的标签。 */
  function open(deviceId: string): void {
    if (!deviceId || openIds.value.includes(deviceId)) return
    openIds.value.push(deviceId)
    persist()
  }

  /** 关闭标签，返回应激活的相邻设备 id（没有则空串）。 */
  function close(deviceId: string): string {
    const i = openIds.value.indexOf(deviceId)
    if (i < 0) return ''
    openIds.value.splice(i, 1)
    persist()
    return openIds.value[Math.min(i, openIds.value.length - 1)] ?? ''
  }

  /** 设备被删除后清掉对应标签。 */
  function prune(knownIds: Set<string>): void {
    const next = openIds.value.filter((id) => knownIds.has(id))
    if (next.length !== openIds.value.length) {
      openIds.value = next
      persist()
    }
  }

  return { openIds, open, close, prune }
})
