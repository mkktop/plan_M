import { reactive } from 'vue'
import { defineStore } from 'pinia'
import { gateway } from '@/api/ws'
import type { TagChange } from '@/api/types'

/** 实时值缓存：只存 UI 当前订阅到的 tag（订阅由页面按需发起）。 */
export const useTagsStore = defineStore('tags', () => {
  const values = reactive(new Map<string, TagChange>())
  /** 本地收到时间：用于展示"多久没更新"，与设备侧 tsMs 区分。 */
  const receivedAt = new Map<string, number>()

  function applyMany(items: TagChange[]): void {
    const now = Date.now()
    for (const it of items) {
      values.set(it.tag, it)
      receivedAt.set(it.tag, now)
    }
  }

  function get(tag: string): TagChange | undefined {
    return values.get(tag)
  }

  function ageMs(tag: string): number {
    const at = receivedAt.get(tag)
    return at === undefined ? Number.POSITIVE_INFINITY : Date.now() - at
  }

  function subscribe(tags: string[]): () => void {
    return gateway.subscribe(tags)
  }

  return { values, applyMany, get, ageMs, subscribe }
})
