<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NButton } from 'naive-ui'
import PageHeader from '@/components/PageHeader.vue'
import AppIcon from '@/components/AppIcon.vue'
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
  { label: '通道', value: String(project.channels.length), hint: '串口总线' },
  {
    label: '设备在线',
    value: `${project.onlineDeviceCount}/${project.devices.length}`,
    hint: '当前轮询正常',
  },
  { label: '设备包', value: String(project.packages.length), hint: '已安装' },
  { label: '实时 tag', value: String(tags.values.size), hint: '本地缓存' },
])

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
  <div class="page">
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
        数据浏览
      </button>
    </div>

    <template v-if="tab === 'health'">
    <div class="grid-2">
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

      <section class="card">
        <div class="card-head">
          <h3>运行统计</h3>
          <span class="muted">来自最近一次快照</span>
        </div>
        <div class="stat-grid">
          <div v-for="s in stats" :key="s.label" class="stat">
            <div class="stat-label">{{ s.label }}</div>
            <div class="stat-value mono">{{ s.value }}</div>
            <div class="stat-hint">{{ s.hint }}</div>
          </div>
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
            <n-button size="tiny" quaternary @click="checkHealth">检测</n-button>
          </div>
        </div>
      </section>
    </div>

    <section class="card log-card">
      <div class="card-head">
        <h3>通信日志</h3>
        <div class="row gap-8">
          <span class="muted small">{{ gs.logs.length }} 条 · 仅保留最近 300 条</span>
          <n-button size="tiny" quaternary @click="gateway.clearLogs()">清空</n-button>
        </div>
      </div>
      <div class="log-list">
        <div v-for="l in gs.logs" :key="l.id" class="log-row" :class="l.dir">
          <span class="log-time mono">{{ formatClock(l.ts) }}</span>
          <span class="log-dir">{{ l.dir === 'in' ? '←' : l.dir === 'out' ? '→' : '·' }}</span>
          <span class="log-text mono">{{ l.text }}</span>
        </div>
        <div v-if="gs.logs.length === 0" class="log-empty muted">暂无消息</div>
      </div>
    </section>
    </template>

    <TagsPage v-else hide-header />
  </div>
</template>
