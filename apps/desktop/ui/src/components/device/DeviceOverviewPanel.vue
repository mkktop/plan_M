<script setup lang="ts">
import { computed, onBeforeUnmount, watch } from 'vue'
import AppIcon from '@/components/AppIcon.vue'
import QualityPill from '@/components/QualityPill.vue'
import EmptyState from '@/components/EmptyState.vue'
import { useProjectStore } from '@/stores/project'
import { useTagsStore } from '@/stores/tags'
import { nowMs } from '@/utils/clock'
import { formatAge } from '@/utils/format'
import { addressLabel, channelStatus, channelSummary } from '@/utils/transport'
import type { Quality } from '@/api/types'

const props = defineProps<{ deviceId: string }>()

const project = useProjectStore()
const tags = useTagsStore()

const device = computed(() => project.devices.find((d) => d.id === props.deviceId) ?? null)
const pkg = computed(() => project.packageOf(device.value))
const channel = computed(() =>
  project.channels.find((c) => c.id === device.value?.channelId),
)

interface TagRow {
  tag: string
  name: string
  quality?: Quality
  ageMs: number
}

const rows = computed<TagRow[]>(() => {
  void nowMs.value // 每秒刷新“多久没更新”
  const d = device.value
  const p = pkg.value
  if (!d || !p) return []
  const out: TagRow[] = []
  for (const pt of p.points) {
    const tag = `${d.id}.${pt.id}`
    out.push({
      tag,
      name: pt.name || pt.id,
      quality: tags.getLive(tag)?.quality,
      ageMs: tags.ageMs(tag),
    })
    for (const b of pt.bits) {
      const bt = `${tag}.b${b.bit}`
      out.push({
        tag: bt,
        name: `${pt.name || pt.id} · ${b.name}`,
        quality: tags.getLive(bt)?.quality,
        ageMs: tags.ageMs(bt),
      })
    }
  }
  return out
})

const total = computed(() => rows.value.length)
const goodCount = computed(() => rows.value.filter((r) => r.quality === 'good').length)
const lostCount = computed(() => rows.value.filter((r) => r.quality === 'comm_lost').length)
const badCount = computed(
  () => rows.value.filter((r) => r.quality === 'bad' || r.quality === 'uncertain').length,
)
const pendingCount = computed(() => total.value - goodCount.value - lostCount.value - badCount.value)
const qualitySegments = computed(() => [
  { label: '良好', tone: 'ok', count: goodCount.value },
  { label: '异常', tone: 'warn', count: badCount.value },
  { label: '失联', tone: 'bad', count: lostCount.value },
  { label: '等待数据', tone: 'idle', count: pendingCount.value },
])
const freshestMs = computed(() =>
  rows.value.reduce((min, r) => Math.min(min, r.ageMs), Number.POSITIVE_INFINITY),
)
const issues = computed(() =>
  rows.value
    .filter((r) => r.quality && r.quality !== 'good')
    .sort((a, b) => a.ageMs - b.ageMs)
    .slice(0, 8),
)

// 概览页也订阅当前设备的 tag，质量统计与“最近更新”才会实时变化。
let dispose: (() => void) | null = null

function resubscribe(): void {
  dispose?.()
  dispose = null
  const d = device.value
  const p = pkg.value
  if (!d || !p) return
  const list: string[] = []
  for (const pt of p.points) {
    list.push(`${d.id}.${pt.id}`)
    for (const b of pt.bits) list.push(`${d.id}.${pt.id}.b${b.bit}`)
  }
  dispose = tags.subscribe(list)
}

watch([() => device.value?.id, pkg], resubscribe, { immediate: true })
onBeforeUnmount(() => dispose?.())
</script>

<template>
  <section v-if="device" class="card dev-overview">
    <div class="card-head">
      <h3>设备状态</h3>
      <span class="state-pill" :class="project.live && device.online ? 'ok' : 'idle'">
        <span class="online-dot" :class="{ on: project.live && device.online }" />
        {{ project.live ? (device.online ? '在线' : '离线') : '状态待确认' }}
      </span>
    </div>

    <div class="stat-grid ov-stats">
      <div class="stat">
        <div class="stat-label">质量良好</div>
        <div class="stat-value mono">{{ project.live ? `${goodCount} / ${total}` : '—' }}</div>
        <div class="stat-hint">当前设备全部点位</div>
      </div>
      <div class="stat">
        <div class="stat-label">失联 / 异常</div>
        <div class="stat-value mono">{{ project.live ? `${lostCount} / ${badCount}` : '—' }}</div>
      </div>
      <div class="stat">
        <div class="stat-label">点位</div>
        <div class="stat-value mono">{{ total }}</div>
        <div class="stat-hint">含位展开子点</div>
      </div>
      <div class="stat">
        <div class="stat-label">最近更新</div>
        <div class="stat-value mono">{{ formatAge(freshestMs) }}</div>
        <div class="stat-hint">距最近一次接收读数</div>
      </div>
    </div>

    <div v-if="project.live && total" class="quality-summary" aria-label="点位质量分布">
      <div class="quality-track" aria-hidden="true">
        <span v-for="segment in qualitySegments" :key="segment.tone" :class="segment.tone" :style="{ flexGrow: segment.count }" />
      </div>
      <div class="quality-legend">
        <span v-for="segment in qualitySegments" :key="segment.tone">
          <span class="state-dot" :class="segment.tone" />{{ segment.label }} <b>{{ segment.count }}</b>
        </span>
      </div>
    </div>

    <div class="ov-meta">
      <dl class="kv-list">
        <div class="kv">
          <dt>设备 ID</dt>
          <dd class="mono">{{ device.id }}</dd>
        </div>
        <div class="kv">
          <dt>设备地址</dt>
          <dd class="mono">{{ addressLabel(device.address) }}</dd>
        </div>
        <div class="kv">
          <dt>通道</dt>
          <dd class="mono">{{ channel?.name || device.channelId }} · {{ channel ? channelSummary(channel) : '—' }}</dd>
        </div>
        <div class="kv">
          <dt>设备包</dt>
          <dd>{{ pkg?.name || device.package }} <span class="mono">v{{ pkg?.version }}</span></dd>
        </div>
        <div class="kv">
          <dt>驱动</dt>
          <dd class="mono">{{ pkg?.protocol.driver || '—' }}</dd>
        </div>
        <div class="kv">
          <dt>通道状态</dt>
          <dd>{{ project.live && channel ? channelStatus(project.channelStates[device.channelId] ?? channel.state).label : '—' }}</dd>
        </div>
      </dl>
    </div>

    <div v-if="project.live && issues.length" class="ov-issues">
      <div class="ov-issues-head">
        <span>需要关注</span>
        <span class="muted small">按最近更新排序 · 最多 8 条</span>
      </div>
      <div v-for="r in issues" :key="r.tag" class="ov-issue">
        <span class="ov-issue-name">{{ r.name }}</span>
        <code class="muted small">{{ r.tag }}</code>
        <QualityPill :quality="r.quality" />
        <span class="muted small mono">{{ formatAge(r.ageMs) }}</span>
      </div>
    </div>
    <div v-else class="banner info ov-ok">
      <AppIcon :name="project.live && total > 0 && !pendingCount ? 'check' : 'activity'" :size="16" />
      <span>{{ !project.live ? '等待网关同步设备状态。' : !total ? '当前设备没有点位。' : pendingCount ? `还有 ${pendingCount} 个点位等待数据。` : '该设备全部点位质量良好。' }}</span>
    </div>
  </section>

  <EmptyState
    v-else
    icon="chip"
    title="设备不存在"
    text="该设备可能已被删除，请返回设备页确认。"
  />
</template>
