<script setup lang="ts">
import { computed } from 'vue'
import type { Quality } from '@/api/types'
import { qualityClass, qualityLabel } from '@/utils/format'

const props = withDefaults(defineProps<{ quality?: Quality; compact?: boolean }>(), {
  quality: undefined,
  compact: false,
})

const cls = computed(() => qualityClass(props.quality))
</script>

<template>
  <span class="quality" :class="[cls, { compact }]">
    <span class="dot" />
    <span v-if="!compact">{{ qualityLabel(quality) }}</span>
  </span>
</template>
