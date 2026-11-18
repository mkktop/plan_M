<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NButton } from 'naive-ui'
import PageHeader from '@/components/PageHeader.vue'
import AppIcon from '@/components/AppIcon.vue'
import MetricCard from '@/components/MetricCard.vue'
import TagsPage from '@/pages/TagsPage.vue'
import { gateway } from '@/api/ws'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { useTagsStore } from '@/stores/tags'
import { useToastStore } from '@/stores/toast'
import { formatClock } from '@/utils/format'

const router = useRouter()
const route = useRoute()
const conn = useConnectionStore()
const project = useProjectStore()
const tags = useTagsStore()
const toast = useToastStore()

// 诊断页 = 连接与健康 + 数据浏览（原全局实时点表：跨设备看值/质量/写值）。
type DiagTab = 'health' | 'data'
const tab = computed<DiagTab>(() => (route.query.tab === 'data' ? 'data' : 'health'))

function setTab(next: DiagTab): void {
  const query = { ...route.query }
  if (next === 'data') query.tab = 'data'
  else delete query.tab
  void router.replace({ query })
}

// 模板里统一走这层别名（Volar 对 class 实例上的 reactive 字段做 ref 解包时类型会跑偏）。
const gs = gateway.state
const uptimeText = computed(() => conn.uptimeText)

const health = ref<{
  state: 'idle' | 'checking' | 'ok' | 'fail'
  latency?: number
  version?: string
  error?: string
  checkedAt?: number
}>({ state: 'idle' })

const healthUrl = computed(() => {
  const raw = gs.url || gs.settings.wsUrl
  try {
    const u = new URL(raw)
    u.protocol = u.protocol === 'wss:' ? 'https:' : 'http:'
    u.pathname = '/health'
    u.search = ''
    return u.toString()
  } catch {
    return ''
  }
})

const stats = computed(() => [
  { label: '连接时长', value: conn.connected ? uptimeText.value : '—', icon: 'clock', tone: 'green' },
  { label: 'HTTP 响应', value: health.value.state === 'ok' ? `${health.value.latency} ms` : '—', icon: 'activity', tone: 'blue' },
  { label: '重连尝试', value: gs.attempts, icon: 'refresh', tone: 'amber' },
  { label: '缓存点位', value: tags.values.size, icon: 'grid', tone: 'violet' },
] as const)

const tokenMasked = computed(() => {
  const t = gs.settings.token
  if (!t) return '未配置'
  return `${t.slice(0, 4)}••••${t.slice(-4)}`
})

async function checkHealth(): Promise<void> {
  if (!healthUrl.value) return
  health.value = { state: 'checking' }
  const t0 = performance.now()
  const ctrl = new AbortController()
  const timer = window.setTimeout(() => ctrl.abort(), 2500)
  try {
    const res = await fetch(healthUrl.value, { signal: ctrl.signal, cache: 'no-store' })
    const latency = Math.round(performance.now() - t0)
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const body = (await res.json()) as { status?: string; version?: string }
    health.value = { state: 'ok', latency, version: body.version, checkedAt: Date.now() }
  } catch (e) {
    const aborted = e instanceof DOMException && e.name === 'AbortError'
    health.value = {
      state: 'fail',
      error: aborted ? '请求超时' : e instanceof Error ? e.message : String(e),
      checkedAt: Date.now(),
    }
  } finally {
    window.clearTimeout(timer)
  }
}

async function reconnect(): Promise<void> {
  await conn.reconnect()
  toast.info('正在重新连接 gatewayd')
}

// —— 通信日志筛选 ——
const logFilter = ref<'all' | 'in' | 'out' | 'sys'>('all')
const LOG_FILTERS = [
  { label: '全部', value: 'all' },
  { label: '下发 →', value: 'out' },
  { label: '← 上行', value: 'in' },
  { label: '系统', value: 'sys' },
] as const

const visibleLogs = computed(() =>
  logFilter.value === 'all' ? gs.logs : gs.logs.filter((l) => l.dir === logFilter.value),
)

function disconnect(): void {
  conn.disconnect()
  toast.info('已断开连接（gatewayd 仍在后台采集）')
}

onMounted(() => {
  void checkHealth()
})

watch(
  () => conn.state.status,
  (s) => {
    if (s === 'connected') void checkHealth()
  },
)
</script>

<template>
  <div class="page diagnostics-page">
    <PageHeader title="诊断" subtitle="连接与链路健康、通信日志，以及跨设备实时数据浏览">
      <n-button size="small" @click="router.push('/settings')">
        <template #icon><AppIcon name="settings" :size="15" /></template>
        连接设置
      </n-button>
      <n-button size="small" type="primary" @click="reconnect">
        <template #icon><AppIcon name="refresh" :size="15" /></template>
        重新连接
      </n-button>
    </PageHeader>

    <div class="seg-tabs" role="tablist">
      <button
        type="button"
        role="tab"
        class="seg-tab"
        :class="{ active: tab === 'health' }"
        :aria-selected="tab === 'health'"
        @click="setTab('health')"
      >
        <AppIcon name="activity" :size="15" />
        连接与健康
      </button>
      <button
        type="button"
        role="tab"
        class="seg-tab"
        :class="{ active: tab === 'data' }"
        :aria-selected="tab === 'data'"
        @click="setTab('data')"
      >
        <AppIcon name="grid" :size="15" />
        跨设备数据浏览
      </button>
    </div>

    <template v-if="tab === 'health'">
    <div class="metric-grid">
      <MetricCard v-for="stat in stats" :key="stat.label" v-bind="stat" />
    </div>
    <div class="grid-2 diagnostics-grid">
      <section class="card conn-card">
        <div class="conn-hero">
          <span class="status-orb" :class="conn.state.status" />
          <div class="conn-main">
            <div class="conn-status">{{ conn.statusLabel }}</div>
            <div class="conn-url mono">{{ gs.url || gs.settings.wsUrl }}</div>
          </div>
          <n-button v-if="conn.connected" size="small" quaternary @click="disconnect">
            <template #icon><AppIcon name="power" :size="15" /></template>
            断开
          </n-button>
        </div>

        <dl class="kv-list">
          <div class="kv">
            <dt>宿主版本</dt>
            <dd class="mono">{{ gs.hostVersion || '—' }}</dd>
          </div>
          <div class="kv">
            <dt>连接时长</dt>
            <dd class="mono">{{ uptimeText }}</dd>
          </div>
          <div class="kv">
            <dt>重连次数</dt>
            <dd class="mono">{{ gs.attempts }}</dd>
          </div>
          <div class="kv">
            <dt>Token</dt>
            <dd class="mono">{{ tokenMasked }}</dd>
          </div>
          <div v-if="gs.info" class="kv">
            <dt>数据目录</dt>
            <dd class="mono ellipsis" :title="gs.info.dataDir">{{ gs.info.dataDir }}</dd>
          </div>
          <div v-if="gs.info" class="kv">
            <dt>Token 文件</dt>
            <dd class="mono ellipsis" :title="gs.info.tokenPath">{{ gs.info.tokenPath }}</dd>
          </div>
        </dl>

        <div v-if="!gs.settings.token" class="banner warn">
          <AppIcon name="alert" :size="16" />
          <span>尚未配置 token。启动 <code>gatewayd serve</code> 后从数据目录读取 token 填入。</span>
        </div>
        <div v-if="gs.status === 'error' && gs.lastError" class="banner danger">
          <AppIcon name="alert" :size="16" />
          <span>{{ gs.lastError }}</span>
        </div>
      </section>

      <section class="card health-card">
        <div class="card-head">
          <div><h3>网关健康检查</h3><p class="muted small">检测网关的 HTTP 响应</p></div>
          <span v-if="health.checkedAt" class="mono muted small">{{ formatClock(health.checkedAt) }}</span>
        </div>

        <div class="health-row">
          <div class="health-left">
            <span class="health-orb" :class="health.state" />
            <div>
              <div class="health-title">HTTP /health</div>
              <div class="mono muted small">{{ healthUrl }}</div>
            </div>
          </div>
          <div class="health-right">
            <template v-if="health.state === 'ok'">
              <span class="text-good">正常 · {{ health.latency }}ms</span>
            </template>
            <template v-else-if="health.state === 'checking'">
              <span class="muted">检测中…</span>
            </template>
            <template v-else-if="health.state === 'fail'">
              <span class="text-bad">{{ health.error }}</span>
            </template>
            <template v-else>
              <span class="muted">未检测</span>
            </template>
            <n-button size="small" :loading="health.state === 'checking'" @click="checkHealth">检测</n-button>
          </div>
        </div>
        <dl class="kv-list">
          <div class="kv"><dt>网关版本</dt><dd class="mono">{{ health.version || '—' }}</dd></div>
          <div class="kv"><dt>已配置设备</dt><dd class="mono">{{ project.loaded ? project.devices.length : '—' }}</dd></div>
          <div class="kv"><dt>已配置通道</dt><dd class="mono">{{ project.loaded ? project.channels.length : '—' }}</dd></div>
        </dl>
      </section>
    </div>

    <section class="card log-card">
      <div class="card-head">
        <h3>通信日志</h3>
        <div class="row gap-8">
          <div class="filter-chips">
            <button
              v-for="f in LOG_FILTERS"
              :key="f.value"
              type="button"
              class="filter-chip"
              :class="{ active: logFilter === f.value }"
              @click="logFilter = f.value"
            >
              {{ f.label }}
            </button>
          </div>
          <span class="muted small">{{ visibleLogs.length }}/{{ gs.logs.length }} 条 · 保留最近 300 条</span>
          <n-button size="tiny" quaternary @click="gateway.clearLogs()">清空</n-button>
        </div>
      </div>
      <div class="log-list">
        <div v-for="l in visibleLogs" :key="l.id" class="log-row" :class="l.dir">
          <span class="log-time mono">{{ formatClock(l.ts) }}</span>
          <span class="log-dir">{{ l.dir === 'in' ? '←' : l.dir === 'out' ? '→' : '·' }}</span>
          <span class="log-text mono">{{ l.text }}</span>
        </div>
        <div v-if="visibleLogs.length === 0" class="log-empty muted">暂无消息</div>
      </div>
    </section>
    </template>

    <TagsPage v-else hide-header />
  </div>
</template>
