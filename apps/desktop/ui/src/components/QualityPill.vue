<script setup lang="ts">
import { computed } from 'vue'
import type { Quality } from '@/api/types'
import { qualityClass, qualityLabel } from '@/utils/format'

const props = withDefaults(defineProps<{ quality?: Quality; compact?: boolean; stale?: boolean }>(), {
  quality: undefined,
  compact: false,
})

const label = computed(() => props.stale ? '缓存' : qualityLabel(props.quality))
const cls = computed(() => qualityClass(props.stale ? undefined : props.quality))
</script>

<template>
  <span class="quality" :class="[cls, { compact }]" :title="compact ? label : undefined">
    <span class="dot" />
    <span v-if="!compact">{{ label }}</span>
  </span>
</template>
