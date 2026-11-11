<script setup lang="ts">
import { useToastStore } from '@/stores/toast'
import AppIcon from './AppIcon.vue'

const toast = useToastStore()
const iconOf = (kind: string) => (kind === 'success' ? 'check' : kind === 'error' ? 'alert' : 'activity')
</script>

<template>
  <Teleport to="body">
    <div class="toast-stack">
      <TransitionGroup name="toast">
        <div v-for="item in toast.items" :key="item.id" class="toast" :class="item.kind" @click="toast.dismiss(item.id)">
          <AppIcon :name="iconOf(item.kind)" :size="16" />
          <span>{{ item.text }}</span>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>
