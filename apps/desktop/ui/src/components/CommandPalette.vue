<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from './AppIcon.vue'
import ModalShell from './ModalShell.vue'
import { useProjectStore } from '@/stores/project'
import { addressLabel } from '@/utils/transport'

interface Command {
  label: string
  detail?: string
  path: string
  icon: string
}

const props = defineProps<{ pages: Command[] }>()
const emit = defineEmits<{ close: [] }>()
const router = useRouter()
const project = useProjectStore()
const query = ref('')
const active = ref(0)
const input = ref<HTMLInputElement>()
const list = ref<HTMLElement>()

const results = computed(() => {
  const q = query.value.trim().toLocaleLowerCase()
  const actions: Command[] = [
    { label: '数据浏览', path: '/diagnostics?tab=data', icon: 'grid' },
    { label: '添加设备', path: '/devices?add=1', icon: 'plus' },
  ]
  const devices = project.devices.map((device) => ({
    label: device.name || device.id,
    detail: `${device.id} · ${addressLabel(device.address)}`,
    path: `/d/${device.id}`,
    icon: 'chip',
  }))
  return [...props.pages, ...actions, ...(q ? devices : [])]
    .filter((command) => `${command.label} ${command.detail ?? ''}`.toLocaleLowerCase().includes(q))
    .slice(0, 16)
})

watch(query, () => { active.value = 0 })
watch(results, (items) => { active.value = Math.max(0, Math.min(active.value, items.length - 1)) })
onMounted(() => input.value?.focus())

function choose(command: Command | undefined): void {
  if (!command) return
  emit('close')
  void router.push(command.path)
}

async function move(step: number): Promise<void> {
  if (!results.value.length) return
  active.value = (active.value + step + results.value.length) % results.value.length
  await nextTick()
  list.value?.children[active.value]?.scrollIntoView({ block: 'nearest' })
}

function onKeydown(event: KeyboardEvent): void {
  if (event.isComposing) return
  switch (event.key) {
    case 'ArrowDown':
    case 'ArrowUp':
      event.preventDefault()
      void move(event.key === 'ArrowDown' ? 1 : -1)
      break
    case 'Enter':
      event.preventDefault()
      choose(results.value[active.value])
      break
  }
}
</script>

<template>
  <ModalShell title="快速跳转" :width="540" @close="emit('close')">
    <div class="command-search">
      <AppIcon name="search" :size="19" />
      <input
        ref="input"
        v-model="query"
        placeholder="搜索页面、操作或设备"
        aria-label="搜索页面、操作或设备"
        role="combobox"
        autocomplete="off"
        aria-controls="command-results"
        :aria-expanded="Boolean(results.length)"
        :aria-activedescendant="results.length ? `command-${active}` : undefined"
        @keydown="onKeydown"
      />
    </div>
    <div id="command-results" ref="list" class="command-results" role="listbox" aria-label="搜索结果">
      <div
        v-for="(command, i) in results"
        :id="`command-${i}`"
        :key="command.path"
        class="command-result"
        :class="{ active: i === active }"
        role="option"
        :aria-selected="i === active"
        @pointerenter="active = i"
        @mousedown.prevent
        @click="choose(command)"
      >
        <span class="command-icon"><AppIcon :name="command.icon" :size="17" /></span>
        <span class="command-label">
          <strong>{{ command.label }}</strong>
          <small v-if="command.detail">{{ command.detail }}</small>
        </span>
        <AppIcon name="arrowRight" :size="15" />
      </div>
    </div>
    <p v-if="!results.length" class="command-empty">没有找到匹配的页面或设备。</p>
    <template #footer>
      <div class="command-help"><kbd>↑</kbd> <kbd>↓</kbd> 选择 <kbd>Enter</kbd> 打开 <kbd>Esc</kbd> 关闭</div>
    </template>
  </ModalShell>
</template>
