<script setup lang="ts">
//! 设备选择器：顶部「+」打开，选中一台设备即打开它的标签页。

import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { NButton, NInput } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import ModalShell from '@/components/ModalShell.vue'
import { useProjectStore } from '@/stores/project'
import { addressLabel, channelSummary } from '@/utils/transport'

const emit = defineEmits<{ (e: 'close'): void; (e: 'pick', deviceId: string): void }>()

const router = useRouter()
const project = useProjectStore()
const keyword = ref('')

function pkgName(packageId: string): string {
  return project.packageById.get(packageId)?.name ?? packageId
}

const groups = computed(() => {
  const kw = keyword.value.trim().toLowerCase()
  return project.channels
    .map((channel) => ({
      channel,
      devices: project.devicesOn(channel.id).filter((d) => {
        if (!kw) return true
        const hay = `${d.name} ${d.id} ${addressLabel(d.address)} ${pkgName(d.package)}`.toLowerCase()
        return hay.includes(kw)
      }),
    }))
    .filter((g) => g.devices.length > 0)
})

const total = computed(() => groups.value.reduce((n, g) => n + g.devices.length, 0))

function pick(id: string): void {
  emit('pick', id)
}

function addDevice(): void {
  void router.push('/devices?add=1')
  emit('close')
}
</script>

<template>
  <ModalShell
    title="选择设备"
    subtitle="每个标签页对应一台设备；选中后会在顶部打开它的标签"
    :width="620"
    @close="emit('close')"
  >
    <div class="picker">
      <n-input v-model:value="keyword" placeholder="搜索名称 / id / 设备地址 / 型号" clearable />

      <div class="picker-list">
        <section v-for="g in groups" :key="g.channel.id" class="picker-group">
          <header class="picker-group-head">
            <AppIcon name="plug" :size="13" />
            <strong>{{ g.channel.name || g.channel.id }}</strong>
            <code class="mono muted small">{{ channelSummary(g.channel) }}</code>
            <span class="spacer" />
            <span class="muted small">{{ g.devices.length }} 台</span>
          </header>

          <button
            v-for="d in g.devices"
            :key="d.id"
            type="button"
            class="picker-item"
            @click="pick(d.id)"
          >
            <span class="online-dot" :class="{ on: d.online }" />
            <span class="picker-item-main">
              <strong>{{ d.name || d.id }}</strong>
              <small class="mono muted">{{ d.id }} · {{ addressLabel(d.address) }}</small>
            </span>
            <span class="picker-item-pkg" :title="pkgName(d.package)">
              {{ pkgName(d.package) }}
            </span>
            <AppIcon name="arrowRight" :size="13" class="picker-item-arrow" />
          </button>
        </section>

        <div v-if="!total && project.loaded" class="picker-empty">
          <AppIcon name="chip" :size="20" />
          <span>{{ keyword ? '没有匹配的设备' : '还没有设备，先添加一台。' }}</span>
          <n-button v-if="!keyword" size="tiny" type="primary" @click="addDevice">新增设备</n-button>
        </div>
      </div>
    </div>

    <template #footer>
      <n-button size="small" @click="emit('close')">取消</n-button>
      <span class="spacer" />
      <n-button size="small" type="primary" @click="addDevice">
        <template #icon><AppIcon name="plus" :size="14" /></template>
        新增设备
      </n-button>
    </template>
  </ModalShell>
</template>
