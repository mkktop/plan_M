<script setup lang="ts">
import { computed, nextTick, ref, useId } from 'vue'
import AppIcon from '@/components/AppIcon.vue'
import type { DeviceWidgetOption } from '@/plugins/deviceWidgets'

const props = defineProps<{ options: DeviceWidgetOption[] }>()
const emit = defineEmits<{ pick: [kind: string] }>()
const id = useId()
const open = ref(false)
const search = ref('')
const searchField = ref<HTMLInputElement>()
const toggle = ref<HTMLButtonElement>()
const category = ref<'全部' | DeviceWidgetOption['category']>('全部')
const categories = computed(() => ['全部', ...new Set(props.options.map((widget) => widget.category))] as const)
const filtered = computed(() => {
  const query = search.value.trim().toLowerCase()
  return props.options.filter((widget) =>
    (category.value === '全部' || widget.category === category.value) &&
    `${widget.label} ${widget.description}`.toLowerCase().includes(query),
  )
})

async function toggleLibrary(): Promise<void> {
  open.value = !open.value
  if (open.value) {
    await nextTick()
    searchField.value?.focus()
  }
}

function select(kind: string): void {
  open.value = false
  search.value = ''
  category.value = '全部'
  emit('pick', kind)
  toggle.value?.focus()
}
</script>

<template>
  <section class="widget-library">
    <div class="widget-library-head">
      <div>
        <h4>组件库</h4>
        <p>为当前页面添加内置组件或插件组件</p>
      </div>
      <button ref="toggle" type="button" class="widget-library-toggle"
        :aria-expanded="open" :aria-controls="id" @click="toggleLibrary">
        <AppIcon :name="open ? 'x' : 'plus'" :size="16" />
        {{ open ? '收起组件库' : '添加组件' }}
      </button>
    </div>
    <div v-if="open" :id="id" class="widget-library-browser">
      <div class="widget-library-toolbar">
        <div class="filter-chips" aria-label="组件分类">
          <button v-for="name in categories" :key="name" type="button" class="filter-chip"
            :class="{ active: category === name }" :aria-pressed="category === name" @click="category = name">{{ name }}</button>
        </div>
        <div class="search-box">
          <AppIcon name="search" :size="14" class="search-icon" />
          <input ref="searchField" v-model="search" type="search" class="search-input"
            aria-label="搜索组件" placeholder="搜索组件" />
        </div>
      </div>
      <div v-if="filtered.length" class="widget-library-grid">
        <button v-for="item in filtered" :key="item.kind" type="button"
          class="widget-library-item" :class="{ extension: !item.core }" @click="select(item.kind)">
          <span class="widget-library-glyph"><AppIcon :name="item.icon" :size="19" /></span>
          <strong>{{ item.title }}</strong>
          <small v-if="item.external">{{ item.external.providerName }}</small>
          <span>{{ item.description }}</span>
          <AppIcon name="plus" :size="15" class="widget-library-plus" />
        </button>
      </div>
      <p v-else class="muted">没有找到匹配的组件。</p>
    </div>
  </section>
</template>
