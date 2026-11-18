<script setup lang="ts">
import { computed, ref } from 'vue'
import AppIcon from '@/components/AppIcon.vue'
import MetricCard from '@/components/MetricCard.vue'
import PageHeader from '@/components/PageHeader.vue'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { addressLabel, channelStatus, transportLabel } from '@/utils/transport'

const conn = useConnectionStore()
const project = useProjectStore()

const channels = computed(() => project.channels.map((channel) => {
  const state = project.channelStates[channel.id] ?? channel.state
  return { ...channel, state, status: channelStatus(state) }
}).sort((a, b) => project.live ? Number(a.state === 'running') - Number(b.state === 'running') : 0))
const runningCount = computed(() => channels.value.filter((channel) => channel.state === 'running').length)
const offlineCount = computed(() => project.devices.length - project.onlineDeviceCount)
const deviceFilter = ref<'all' | 'offline' | null>(null)
const activeFilter = computed(() => project.live ? deviceFilter.value ?? (offlineCount.value ? 'offline' : 'all') : 'all')
const devices = computed(() => {
  const all = activeFilter.value === 'offline'
    ? project.devices.filter((device) => !device.online)
    : project.devices
  return all.slice(0, 5)
})

const endpoint = computed(() => {
  try {
    return new URL(conn.state.url || conn.state.settings.wsUrl).host
  } catch {
    return '—'
  }
})
</script>

<template>
  <div class="page home-page">
    <PageHeader title="运行总览" subtitle="查看采集链路、设备在线情况与实时点位">
      <router-link class="action-link primary" :to="project.loaded && !project.devices.length ? '/devices?add=1' : '/devices'">
        <AppIcon name="chip" :size="16" /> {{ project.loaded && !project.devices.length ? '接入设备' : '管理设备' }}
      </router-link>
    </PageHeader>

    <section class="card home-gateway" aria-label="采集网关">
      <span class="inventory-icon"><AppIcon name="server" :size="20" /></span>
      <div class="gateway-identity">
        <h3>采集网关</h3>
        <span class="mono muted small">{{ endpoint }}</span>
      </div>
      <span class="state-pill" :class="project.live ? 'ok' : 'warn'">
        {{ conn.connected && !project.live ? '同步中' : conn.statusLabel }}
      </span>
      <span v-if="conn.connected && conn.uptimeMs" class="muted small">已连接 {{ conn.uptimeText }}</span>
      <router-link to="/diagnostics" class="panel-link">
        查看连接诊断 <AppIcon name="arrowRight" :size="15" />
      </router-link>
    </section>

    <section class="metric-grid" aria-label="运行指标">
      <MetricCard label="在线设备" :value="project.live ? project.onlineDeviceCount : '—'"
        :total="project.loaded ? project.devices.length : undefined"
        :detail="project.live ? `${offlineCount} 台离线` : '等待网关状态'"
        icon="chip" :tone="project.live && offlineCount ? 'amber' : 'blue'" :to="project.live && offlineCount ? '/devices?status=offline' : '/devices'" />
      <MetricCard label="运行通道" :value="project.live ? runningCount : '—'"
        :total="project.loaded ? channels.length : undefined" detail="运行中的通道"
        icon="network" tone="blue" to="/devices" />
      <MetricCard label="配置采集点" :value="project.loaded ? project.totalPointCount : '—'"
        detail="按设备统计 · 含位展开子点" icon="chart" tone="amber" to="/diagnostics?tab=data" />
      <MetricCard label="已安装设备包" :value="project.loaded ? project.packages.length : '—'"
        detail="查看设备型号与点表" icon="package" tone="violet" to="/packages" />
    </section>

    <div class="home-grid">
      <section class="card home-inventory">
        <header class="home-panel-header">
          <div><h3>设备概览</h3><p>{{ project.loaded ? `${project.devices.length} 台已配置设备` : '等待设备清单' }}</p></div>
          <router-link class="panel-link" to="/devices">全部设备 <AppIcon name="arrowRight" :size="14" /></router-link>
        </header>
        <div v-if="project.devices.length" class="inventory-toolbar">
          <div class="filter-chips" aria-label="筛选设备">
            <button type="button" class="filter-chip" :class="{ active: activeFilter === 'all' }"
              :aria-pressed="activeFilter === 'all'" @click="deviceFilter = 'all'">全部</button>
            <button type="button" class="filter-chip" :class="{ active: activeFilter === 'offline' }"
              :aria-pressed="activeFilter === 'offline'" :disabled="!project.live"
              @click="deviceFilter = 'offline'">离线 {{ project.live ? offlineCount : '—' }}</button>
          </div>
          <span v-if="devices.length" class="muted small">显示 {{ devices.length }} 台</span>
        </div>
        <div v-if="devices.length" class="inventory-list">
          <router-link v-for="device in devices" :key="device.id" :to="`/d/${device.id}`" class="inventory-row">
            <span class="inventory-icon"><AppIcon name="chip" :size="17" /></span>
            <span class="inventory-name">
              <strong>{{ device.name || device.id }}</strong>
              <small>{{ project.channelOf(device)?.name || device.channelId }} · {{ addressLabel(device.address) }}</small>
            </span>
            <span class="inventory-package">{{ project.packageOf(device)?.name || device.package }}</span>
            <span class="state-pill" :class="!project.live ? 'idle' : device.online ? 'ok' : 'warn'">
              {{ project.live ? (device.online ? '在线' : '离线') : '待确认' }}
            </span>
            <AppIcon class="inventory-arrow" name="arrowUpRight" :size="15" />
          </router-link>
        </div>
        <div v-else class="home-placeholder">
          <span class="inventory-icon"><AppIcon :name="project.devices.length ? 'check' : 'chip'" :size="20" /></span>
          <strong>{{ !project.loaded ? '设备清单尚未同步' : project.devices.length ? '没有离线设备' : '还没有设备' }}</strong>
        </div>
      </section>

      <section class="card home-health">
        <header class="home-panel-header">
          <div><h3>采集通道</h3><p v-if="project.live && runningCount < channels.length">{{ channels.length - runningCount }} 条通道未运行</p></div>
          <router-link class="panel-link" to="/devices">全部通道 <AppIcon name="arrowRight" :size="14" /></router-link>
        </header>
        <div class="health-channels">
          <router-link v-for="channel in channels.slice(0, 5)" :key="channel.id" class="health-channel"
            :to="{ path: '/devices', query: { channel: channel.id } }">
            <AppIcon name="plug" :size="16" />
            <span :title="channel.name || channel.id">{{ channel.name || channel.id }}</span>
            <small>{{ transportLabel(channel.transport) }}</small>
            <span class="state-pill" :class="project.live ? channel.status.tone : 'idle'">{{ project.live ? channel.status.label : '待确认' }}</span>
          </router-link>
          <div v-if="!channels.length" class="health-channel-empty muted">
            {{ project.loaded ? '尚未配置采集通道' : '等待通道清单' }}
          </div>
        </div>
      </section>
    </div>

  </div>
</template>
