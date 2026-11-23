<script setup lang="ts">
import AppIcon from './AppIcon.vue'

defineProps<{
  label: string
  value: number | string
  total?: number
  detail?: string
  icon: string
  to?: string
  tone?: 'green' | 'blue' | 'amber' | 'violet'
}>()
</script>

<template>
  <component :is="to ? 'router-link' : 'div'" :to="to" class="metric-card" :class="tone">
    <span class="metric-head">
      <span class="metric-icon"><AppIcon :name="icon" :size="18" /></span>
      <span class="metric-label">{{ label }}</span>
      <AppIcon v-if="to" class="metric-arrow" name="arrowUpRight" :size="15" />
    </span>
    <strong class="metric-value">
      {{ value }}<small v-if="total !== undefined"> / {{ total }}</small>
    </strong>
    <span v-if="detail" class="metric-detail">{{ detail }}</span>
  </component>
</template>
