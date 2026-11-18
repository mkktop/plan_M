import { reactive } from 'vue'
import { defineStore } from 'pinia'
import { gateway } from '@/api/ws'
import type { TagChange } from '@/api/types'

/** 实时值缓存：只存 UI 当前订阅到的 tag（订阅由页面按需发起）。 */
export const useTagsStore = defineStore('tags', () => {
  const values = reactive(new Map<string, TagChange>())
  /** 本地收到时间：用于展示"多久没更新"，与设备侧 tsMs 区分。 */
  const received = new Map<string, { at: number; session: number; sampleAt?: number }>()

  function applyMany(items: TagChange[]): void {
    const now = Date.now()
    for (const it of items) {
      // 失联事件保留旧读数，但消息时间是失联时间。
      const sampleAt = it.quality === 'comm_lost' ? received.get(it.tag)?.sampleAt : it.tsMs
      received.set(it.tag, { at: now, session: gateway.state.session, sampleAt })
      values.set(it.tag, it)
    }
  }

  function get(tag: string): TagChange | undefined {
    return values.get(tag)
  }

  function getLive(tag: string): TagChange | undefined {
    const value = get(tag)
    return gateway.state.status === 'connected' && received.get(tag)?.session === gateway.state.session
      ? value
      : undefined
  }

  function ageMs(tag: string): number {
    const at = received.get(tag)?.at
    return at === undefined ? Number.POSITIVE_INFINITY : Date.now() - at
  }

  function sampleTime(tag: string): number | undefined {
    return received.get(tag)?.sampleAt
  }

  function subscribe(tags: string[]): () => void {
    return gateway.subscribe(tags)
  }

  return { values, applyMany, get, getLive, ageMs, sampleTime, subscribe }
})
