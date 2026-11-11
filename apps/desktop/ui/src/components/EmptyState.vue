<script setup lang="ts">
import AppIcon from './AppIcon.vue'

withDefaults(defineProps<{ icon?: string; title: string; text?: string }>(), {
  icon: 'grid',
  text: '',
})
</script>

<template>
  <div class="empty-state">
    <div class="empty-icon">
      <AppIcon :name="icon" :size="24" />
    </div>
    <h3>{{ title }}</h3>
    <p v-if="text" class="muted">{{ text }}</p>
    <div v-if="$slots.default" class="empty-actions">
      <slot />
    </div>
  </div>
</template>
