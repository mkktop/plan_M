<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue'
import AppIcon from './AppIcon.vue'

withDefaults(defineProps<{ title: string; subtitle?: string; width?: number }>(), {
  subtitle: '',
  width: 560,
})

const emit = defineEmits<{ (e: 'close'): void }>()

function onKey(e: KeyboardEvent): void {
  if (e.key === 'Escape') emit('close')
}

let prevOverflow = ''
onMounted(() => {
  prevOverflow = document.body.style.overflow
  document.body.style.overflow = 'hidden'
  window.addEventListener('keydown', onKey)
})
onBeforeUnmount(() => {
  document.body.style.overflow = prevOverflow
  window.removeEventListener('keydown', onKey)
})
</script>

<template>
  <Teleport to="body">
    <div class="modal-mask" @mousedown.self="emit('close')">
      <div class="modal" :style="{ width: width + 'px' }" role="dialog" aria-modal="true">
        <header class="modal-head">
          <div class="modal-title">
            <h2>{{ title }}</h2>
            <p v-if="subtitle" class="muted">{{ subtitle }}</p>
          </div>
          <button type="button" class="icon-btn" title="关闭" @click="emit('close')">
            <AppIcon name="x" :size="18" />
          </button>
        </header>
        <div class="modal-body">
          <slot />
        </div>
        <footer v-if="$slots.footer" class="modal-foot">
          <slot name="footer" />
        </footer>
      </div>
    </div>
  </Teleport>
</template>
