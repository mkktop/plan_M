<script setup lang="ts">
import { computed } from 'vue'
import { formatNumber } from '@/utils/format'
import { useNumericSamples } from '@/utils/liveSamples'
import type { TagChange } from '@/api/types'

const props = defineProps<{ change?: TagChange; connected: boolean; decimals: number }>()
const samples = useNumericSamples(() => props.change, () => props.connected)

const range = computed(() => ({
  min: Math.min(...samples.value.map((sample) => sample.value)),
  max: Math.max(...samples.value.map((sample) => sample.value)),
}))
const points = computed(() => {
  const first = samples.value[0]
  const last = samples.value.at(-1)
  if (!first || !last || first === last) return []
  const elapsed = last.time - first.time
  const span = range.value.max - range.value.min
  return samples.value.map(({ time, value }) => ({
    x: 2 + ((time - first.time) / elapsed) * 236,
    y: span ? 57 - ((value - range.value.min) / span) * 50 : 32,
  }))
})
const line = computed(() => points.value.map(({ x, y }) => `${x},${y}`).join(' '))
</script>

<template>
  <div class="live-trend">
    <svg v-if="points.length" viewBox="0 0 240 64" preserveAspectRatio="none" role="img" aria-label="最近有效点位数据的实时趋势">
      <path class="trend-guides" d="M0 8h240 M0 32h240 M0 56h240" />
      <polygon class="trend-area" :points="`2,64 ${line} 238,64`" />
      <polyline class="trend-line" :points="line" />
    </svg>
    <div v-else class="trend-placeholder">{{ connected ? '等待有效采样' : '等待网关连接' }}</div>
    <div v-if="points.length" class="trend-caption">
      <span>最近 {{ samples.length }} 次更新</span>
      <span class="mono">{{ formatNumber(range.min, decimals) }} – {{ formatNumber(range.max, decimals) }}</span>
    </div>
  </div>
</template>
