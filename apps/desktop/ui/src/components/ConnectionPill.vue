<script setup lang="ts">
import { computed } from 'vue'
import { useConnectionStore } from '@/stores/connection'

const conn = useConnectionStore()
const label = computed(() => conn.statusLabel)
</script>

<template>
  <button type="button" class="conn-pill" :class="conn.state.status" :title="conn.state.lastError || conn.state.url">
    <span class="dot" />
    <span>{{ label }}</span>
    <span v-if="conn.state.hostVersion" class="conn-ver mono">v{{ conn.state.hostVersion }}</span>
  </button>
</template>
