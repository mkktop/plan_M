<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, useId } from 'vue'
import AppIcon from './AppIcon.vue'

withDefaults(defineProps<{ title: string; subtitle?: string; width?: number }>(), {
  subtitle: '',
  width: 560,
})

const emit = defineEmits<{ (e: 'close'): void }>()
const titleId = useId()
const dialog = ref<HTMLElement>()
let returnFocus: HTMLElement | null = null

function onKey(e: KeyboardEvent): void {
  if (e.key === 'Escape' && !e.isComposing && !e.defaultPrevented) emit('close')
}

let prevOverflow = ''
onMounted(() => {
  returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
  prevOverflow = document.body.style.overflow
  document.body.style.overflow = 'hidden'
  window.addEventListener('keydown', onKey)
  void nextTick(() => {
    if (!dialog.value?.contains(document.activeElement)) dialog.value?.focus()
  })
})
onBeforeUnmount(() => {
  document.body.style.overflow = prevOverflow
  window.removeEventListener('keydown', onKey)
  if (returnFocus?.isConnected) returnFocus.focus()
})
</script>

<template>
  <Teleport to="body">
    <div class="modal-mask" @mousedown.self="emit('close')">
      <div ref="dialog" class="modal" :style="{ width: width + 'px' }" role="dialog" aria-modal="true" :aria-labelledby="titleId" tabindex="-1">
        <header class="modal-head">
          <div class="modal-title">
            <h2 :id="titleId">{{ title }}</h2>
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
