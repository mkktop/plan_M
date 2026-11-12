<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { NButton } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import { useConnectionStore } from '@/stores/connection'
import { usePluginsStore } from '@/stores/plugins'
import { useProjectStore } from '@/stores/project'
import { useTagsStore } from '@/stores/tags'
import { nowMs } from '@/utils/clock'
import { formatClock } from '@/utils/format'

const router = useRouter()
const conn = useConnectionStore()
const project = useProjectStore()
const plugins = usePluginsStore()
const tags = useTagsStore()

const greeting = computed(() => {
  const h = new Date(nowMs.value).getHours()
  if (h < 5) return '夜深了'
  if (h < 11) return '早上好'
  if (h < 14) return '中午好'
  if (h < 18) return '下午好'
  return '晚上好'
})

const clockText = computed(() => formatClock(nowMs.value))
const dateText = computed(() =>
  new Date(nowMs.value).toLocaleDateString('zh-CN', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    weekday: 'long',
  }),
)

const runningChannels = computed(
  () => project.channels.filter((c) => project.channelStates[c.id] === 'running').length,
)

const stats = computed(() => [
  {
    label: '在线设备',
    value: `${project.onlineDeviceCount}/${project.devices.length}`,
    hint: '当前轮询正常',
    icon: 'chip',
  },
  {
    label: '采集通道',
    value: `${runningChannels.value}/${project.channels.length}`,
    hint: '运行中 / 已配置',
    icon: 'plug',
  },
  {
    label: '点位',
    value: String(project.totalPointCount),
    hint: `${tags.values.size} 个实时值已缓存`,
    icon: 'grid',
  },
  {
    label: '设备包',
    value: String(project.packages.length),
    hint: '已安装型号',
    icon: 'package',
  },
  {
    label: 'UI 插件',
    value: String(plugins.list.length),
    hint: 'ESM 扩展',
    icon: 'plugin',
  },
])

const actions = [
  { path: '/devices', label: '管理设备', desc: '总线、从站与扫描', icon: 'chip' },
  { path: '/diagnostics?tab=data', label: '数据浏览', desc: '跨设备实时值、质量与写值', icon: 'grid' },
  { path: '/packages', label: '设备包', desc: '导入新型号与点表', icon: 'package' },
  { path: '/plugins', label: 'UI 插件', desc: '扩展仪表盘组件与页面', icon: 'plugin' },
]

const recentDevices = computed(() => project.devices.slice(0, 5))

function packageName(id: string): string {
  return project.packageById.get(id)?.name ?? id
}
</script>

<template>
  <div class="page home-page">
    <section class="home-hero">
      <div class="home-hero-main">
        <span class="home-kicker">PLAN M · INDUSTRIAL HMI</span>
        <h2 class="home-hello">Hello<span class="home-hello-dot">.</span></h2>
        <p class="home-sub">
          {{ greeting }}，今天是 {{ dateText }}。
          <template v-if="conn.connected">
            采集链路正常，{{ project.onlineDeviceCount }} 台设备在线。
          </template>
          <template v-else>正在等待 gatewayd 连接，采集进程未就绪。</template>
        </p>
        <div class="home-hero-actions">
          <n-button size="medium" type="primary" @click="router.push('/devices')">
            <template #icon><AppIcon name="chip" :size="16" /></template>
            管理设备
          </n-button>
          <n-button size="medium" @click="router.push('/diagnostics?tab=data')">
            <template #icon><AppIcon name="grid" :size="16" /></template>
            数据浏览
          </n-button>
        </div>
      </div>

      <div class="home-hero-side">
        <div class="home-status">
          <span class="status-orb" :class="conn.state.status" />
          <div class="home-status-text">
            <strong>{{ conn.statusLabel }}</strong>
            <span class="mono">{{ conn.state.url || conn.state.settings.wsUrl }}</span>
          </div>
          <span class="state-pill" :class="conn.connected ? 'ok' : 'idle'">
            v{{ conn.state.hostVersion || '0.1.0' }}
          </span>
        </div>
        <div class="home-clock mono">{{ clockText }}</div>
        <div class="home-date">{{ dateText }}</div>
      </div>

      <div class="home-orbit" aria-hidden="true">
        <span /><span /><span />
      </div>
    </section>

    <section class="home-stats">
      <article v-for="s in stats" :key="s.label" class="home-stat">
        <div class="home-stat-head">
          <span>{{ s.label }}</span>
          <AppIcon :name="s.icon" :size="15" />
        </div>
        <strong class="mono">{{ s.value }}</strong>
        <small>{{ s.hint }}</small>
      </article>
    </section>

    <div class="home-columns">
      <section class="card home-panel">
        <div class="card-head">
          <h3>快速开始</h3>
          <span class="muted small">常用入口</span>
        </div>
        <div class="home-actions">
          <button
            v-for="a in actions"
            :key="a.path"
            type="button"
            class="home-action"
            @click="router.push(a.path)"
          >
            <span class="home-action-icon"><AppIcon :name="a.icon" :size="17" /></span>
            <span class="home-action-text">
              <strong>{{ a.label }}</strong>
              <small>{{ a.desc }}</small>
            </span>
            <AppIcon name="arrowRight" :size="14" class="home-action-arrow" />
          </button>
        </div>
      </section>

      <section class="card home-panel">
        <div class="card-head">
          <h3>设备概览</h3>
          <router-link class="home-more" to="/devices">全部设备</router-link>
        </div>

        <div v-if="recentDevices.length" class="home-devices">
          <div v-for="d in recentDevices" :key="d.id" class="home-device">
            <span class="online-dot" :class="{ on: d.online }" />
            <div class="home-device-main">
              <strong>{{ d.name }}</strong>
              <span class="mono muted small">{{ d.id }} · 从站 #{{ d.slave }}</span>
            </div>
            <span class="home-device-pkg" :title="packageName(d.package)">
              {{ packageName(d.package) }}
            </span>
            <span class="state-pill" :class="d.online ? 'ok' : 'idle'">
              {{ d.online ? '在线' : '离线' }}
            </span>
          </div>
        </div>

        <div v-else class="home-empty">
          <AppIcon name="chip" :size="20" />
          <span>还没有设备，导入设备包后添加从站。</span>
          <n-button size="tiny" type="primary" @click="router.push('/devices')">
            添加设备
          </n-button>
        </div>

        <div v-if="recentDevices.length" class="home-device-foot">
          <span><b>{{ project.onlineDeviceCount }}</b> 在线</span>
          <span><b>{{ project.devices.length }}</b> 台已配置</span>
          <span class="spacer" />
          <span>{{ project.channels.length }} 条总线</span>
        </div>
      </section>
    </div>
  </div>
</template>
