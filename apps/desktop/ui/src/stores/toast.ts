import { ref } from 'vue'
import { defineStore } from 'pinia'

export type ToastKind = 'success' | 'error' | 'info'

export interface ToastItem {
  id: number
  kind: ToastKind
  text: string
}

/** 轻量提示条：避免为几个提示引入全局 dialog provider。 */
export const useToastStore = defineStore('toast', () => {
  const items = ref<ToastItem[]>([])
  let seq = 0

  function dismiss(id: number): void {
    items.value = items.value.filter((i) => i.id !== id)
  }

  function push(kind: ToastKind, text: string, ttl = 4200): number {
    const id = ++seq
    items.value.push({ id, kind, text })
    window.setTimeout(() => dismiss(id), ttl)
    return id
  }

  return {
    items,
    dismiss,
    push,
    success: (t: string) => push('success', t),
    error: (t: string) => push('error', t, 6000),
    info: (t: string) => push('info', t),
  }
})

export function errorMessage(e: unknown): string {
  if (e instanceof Error) return e.message
  return String(e)
}
