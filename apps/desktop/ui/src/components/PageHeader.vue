<script setup lang="ts">
defineProps<{ title: string; subtitle?: string }>()
</script>

<template>
  <header class="page-head">
    <div>
      <h2>{{ title }}</h2>
      <p v-if="subtitle" class="muted">{{ subtitle }}</p>
    </div>
    <div class="page-actions">
      <slot />
    </div>
  </header>
</template>
