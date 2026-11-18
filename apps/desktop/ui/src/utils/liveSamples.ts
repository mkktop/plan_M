import { shallowRef, watch } from 'vue'
import { primitiveOf } from '@/utils/format'
import type { TagChange } from '@/api/types'

/** Keep a bounded window of good samples from the current connection and binding. */
export function useNumericSamples(change: () => TagChange | undefined, connected: () => boolean) {
  const samples = shallowRef<{ time: number; value: number }[]>([])
  let activeTag = ''
  watch(() => [change(), connected()] as const, ([sample, live]) => {
    const value = primitiveOf(sample?.value)
    if (!live || !sample || sample.quality !== 'good' || typeof value !== 'number' || !Number.isFinite(value)) {
      samples.value = []
      return
    }
    const last = samples.value.at(-1)
    const row = { time: sample.tsMs, value }
    if (!last || sample.tag !== activeTag || sample.tsMs < last.time) {
      samples.value = [row]
    } else {
      const previous = last.time === sample.tsMs ? samples.value.slice(0, -1) : samples.value
      samples.value = [...previous.slice(-59), row]
    }
    activeTag = sample.tag
  }, { immediate: true })
  return samples
}
