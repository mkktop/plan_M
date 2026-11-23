<script setup lang="ts">
defineProps<{ title: string; subtitle?: string }>()
</script>

<template>
  <header class="page-head">
    <div>
      <h1>{{ title }}</h1>
      <p v-if="subtitle" class="muted">{{ subtitle }}</p>
    </div>
    <div v-if="$slots.default" class="page-actions">
      <slot />
    </div>
  </header>
</template>
