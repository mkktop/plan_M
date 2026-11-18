<script setup lang="ts">
import { useToastStore } from '@/stores/toast'
import AppIcon from './AppIcon.vue'

const toast = useToastStore()
const iconOf = (kind: string) => (kind === 'success' ? 'check' : kind === 'error' ? 'alert' : 'activity')
</script>

<template>
  <Teleport to="body">
    <!-- aria-live：屏幕阅读器播报，不打断焦点 -->
    <div class="toast-stack" aria-live="polite">
      <TransitionGroup name="toast">
        <div
          v-for="item in toast.items"
          :key="item.id"
          class="toast"
          :class="item.kind"
          role="status"
          @click="toast.dismiss(item.id)"
        >
          <AppIcon :name="iconOf(item.kind)" :size="16" />
          <span>{{ item.text }}</span>
          <!-- 自动关闭倒计时条：时长由该条 toast 的 ttl 驱动 -->
          <span class="toast-life" :style="{ '--toast-life': `${item.ttl}ms` }" />
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>
