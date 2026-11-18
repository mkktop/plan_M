<script setup lang="ts">
import { computed } from 'vue'
import { formatNumber } from '@/utils/format'
import { useNumericSamples } from '@/utils/liveSamples'
import type { TagChange } from '@/api/types'

const props = defineProps<{ change?: TagChange; connected: boolean; decimals: number; unit: string }>()
const samples = useNumericSamples(() => props.change, () => props.connected)
const values = computed(() => {
  const rows = samples.value.map((row) => row.value)
  if (!rows.length) return null
  return [
    { label: '最小', value: Math.min(...rows) },
    { label: '均值', value: rows.reduce((sum, value) => sum + value / rows.length, 0) },
    { label: '最大', value: Math.max(...rows) },
  ]
})
</script>

<template>
  <div v-if="values" class="sample-stats">
    <dl>
      <div v-for="stat in values" :key="stat.label">
        <dt>{{ stat.label }}</dt>
        <dd :title="formatNumber(stat.value, decimals) + (unit ? ' ' + unit : '')">
          {{ formatNumber(stat.value, decimals) }}<small v-if="unit"> {{ unit }}</small>
        </dd>
      </div>
    </dl>
    <p>最近 {{ samples.length }} 次有效更新</p>
  </div>
  <div v-else class="dev-widget-empty">{{ connected ? '等待有效读数' : '等待网关连接' }}</div>
</template>
