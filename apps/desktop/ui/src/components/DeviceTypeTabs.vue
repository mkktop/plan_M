<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppIcon from '@/components/AppIcon.vue'
import { useProjectStore } from '@/stores/project'

const route = useRoute()
const router = useRouter()
const project = useProjectStore()

const packages = computed(() =>
  [...project.packages].sort((a, b) => a.name.localeCompare(b.name, 'zh-CN')),
)

const activeTypeId = computed(() =>
  typeof route.params.typeId === 'string' ? route.params.typeId : '',
)

function deviceCount(packageId: string): number {
  return project.devices.filter((d) => d.package === packageId).length
}

function openType(packageId: string): void {
  if (route.params.typeId === packageId) return
  void router.push(`/t/${packageId}`)
}
</script>

<template>
  <div v-if="packages.length" class="type-tabs">
    <button
      v-for="p in packages"
      :key="p.id"
      type="button"
      class="type-tab"
      :class="{ active: activeTypeId === p.id }"
      :title="`${p.name} · ${p.id}`"
      @click="openType(p.id)"
    >
      <span class="type-tab-icon"><AppIcon name="package" :size="15" /></span>
      <span class="type-tab-name">{{ p.name }}</span>
      <span v-if="deviceCount(p.id)" class="type-tab-count mono">{{ deviceCount(p.id) }}</span>
    </button>

    <button type="button" class="type-tab-plus" title="导入 / 管理设备包" @click="router.push('/packages')">
      <AppIcon name="plus" :size="15" />
    </button>

    <span class="spacer" />
    <button type="button" class="type-tabs-manage" @click="router.push('/packages')">
      <AppIcon name="package" :size="14" />
      设备包
    </button>
  </div>

  <div v-else-if="project.loaded" class="type-tabs empty">
    <span class="muted small">导入设备包后，这里会出现设备类型标签。</span>
    <button type="button" class="type-tabs-manage" @click="router.push('/packages')">
      去导入设备包
    </button>
  </div>
</template>
