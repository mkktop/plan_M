<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NButton, NInput, NInputNumber, NSwitch } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import EmptyState from '@/components/EmptyState.vue'
import ModalShell from '@/components/ModalShell.vue'
import PageHeader from '@/components/PageHeader.vue'
import QualityPill from '@/components/QualityPill.vue'
import { gateway } from '@/api/ws'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { useTagsStore } from '@/stores/tags'
import { errorMessage, useToastStore } from '@/stores/toast'
import { nowMs } from '@/utils/clock'
import { effectivePoint } from '@/utils/devicePoints'
import type { BitDto, PointDto, Quality, TagValue } from '@/api/types'
import {
  boolOf,
  formatAge,
  formatClock,
  isBitArea,
  primitiveOf,
  qualityClass,
  typeLabel,
  valueText,
} from '@/utils/format'
import { addressLabel, addressText, channelSummary } from '@/utils/transport'

interface BitRow extends BitDto {
  value?: boolean
  quality?: Quality
  stale: boolean
  text: string
}

interface Row {
  point: PointDto
  tag: string
  quality?: Quality
  valueText: string
  sampleAt?: number
  stale: boolean
  bits: BitRow[]
}

const route = useRoute()
const router = useRouter()
const project = useProjectStore()
const conn = useConnectionStore()
const tags = useTagsStore()
const toast = useToastStore()

const props = withDefaults(
  defineProps<{
    embedded?: boolean
    deviceId?: string
    /** 嵌在诊断页的标签里：不要自己的页头，但保留设备选择侧栏。 */
    hideHeader?: boolean
  }>(),
  { embedded: false, deviceId: '', hideHeader: false },
)

const selectedId = ref('')
const deviceFilter = ref('')
const pointFilter = ref('')
const issuesOnly = ref(false)

const selected = computed(() => project.devices.find((d) => d.id === selectedId.value) ?? null)
const selectedPkg = computed(() => project.packageOf(selected.value))
const selectedChannel = computed(() =>
  project.channels.find((c) => c.id === selected.value?.channelId),
)

const filteredDevices = computed(() => {
  const q = deviceFilter.value.trim().toLowerCase()
  if (!q) return project.devices
  return project.devices.filter(
    (d) => d.id.toLowerCase().includes(q) || d.name.toLowerCase().includes(q),
  )
})

const rows = computed<Row[]>(() => {
  const d = selected.value
  const pkg = selectedPkg.value
  if (!d || !pkg) return []
  return pkg.points.map((pt) => {
    const tag = `${d.id}.${pt.id}`
    const cur = tags.get(tag)
    const live = tags.getLive(tag)
    const sampleAt = tags.sampleTime(tag)
    return {
      point: effectivePoint(d, pt),
      tag,
      quality: live?.quality,
      valueText: valueText(sampleAt === undefined ? undefined : cur?.value),
      sampleAt,
      stale: sampleAt !== undefined && !live,
      bits: pt.bits.map((b) => {
        const bitTag = `${tag}.b${b.bit}`
        const current = tags.getLive(bitTag)
        const hasSample = tags.sampleTime(bitTag) !== undefined
        const v = boolOf(hasSample ? tags.get(bitTag)?.value : undefined)
        return {
          ...b,
          value: v,
          quality: current?.quality,
          stale: hasSample && !current,
          text: v === undefined ? '—' : v ? b.on || 'ON' : b.off || 'OFF',
        }
      }),
    }
  })
})

const pendingCount = computed(() => rows.value.filter((row) => row.stale).length)

const filteredRows = computed(() => {
  const q = pointFilter.value.trim().toLowerCase()
  if (!q && !issuesOnly.value) return rows.value
  return rows.value.filter((row) => {
    const matches = !q || [row.point.id, row.point.name, ...row.bits.map((bit) => bit.name)]
      .some((value) => value.toLowerCase().includes(q))
    const hasIssue = row.stale || [row.quality, ...row.bits.map((bit) => bit.quality)]
      .some((quality) => quality && quality !== 'good')
    return matches && (!issuesOnly.value || hasIssue)
  })
})

const tagCount = computed(() => rows.value.reduce((n, r) => n + 1 + r.bits.length, 0))
const allQualities = computed<Array<Quality | undefined>>(() =>
  rows.value.flatMap((r) => [r.quality, ...r.bits.map((b) => b.quality)]),
)
const goodCount = computed(() => allQualities.value.filter((q) => q === 'good').length)
const lostCount = computed(() => allQualities.value.filter((q) => q === 'comm_lost').length)

// —— 数值变化闪动：对比前后快照，变化的主值短暂加 .flash ——
const valueSnapshot = computed(() => {
  const m = new Map<string, string>()
  for (const r of rows.value) m.set(r.tag, r.valueText)
  return m
})

const flashing = ref(new Set<string>())
const flashTimers = new Map<string, ReturnType<typeof setTimeout>>()

watch(valueSnapshot, (cur, prev) => {
  if (!prev) return
  for (const [tag, text] of cur) {
    if (text !== '—' && prev.get(tag) !== text) {
      flashing.value.add(tag)
      const old = flashTimers.get(tag)
      if (old) clearTimeout(old)
      flashTimers.set(
        tag,
        setTimeout(() => {
          flashing.value.delete(tag)
          flashTimers.delete(tag)
        }, 600),
      )
    }
  }
})

const writeOpen = ref(false)
const writeTarget = ref<Row | null>(null)
const writeNumber = ref<number | null>(null)
const writeBool = ref(false)
const writing = ref(false)
const writeError = ref('')
watch([selectedId, () => project.live], () => {
  writeOpen.value = false
})

const isBoolPoint = computed(() =>
  writeTarget.value ? isBitArea(writeTarget.value.point.area) : false,
)

const writeHint = computed(() => {
  const p = writeTarget.value?.point
  if (!p) return ''
  if (isBoolPoint.value) return '线圈写入布尔值（1/0）。'
  const parts: string[] = []
  if (p.scale !== null) parts.push(`scale ${p.scale}`)
  if (p.offset !== null) parts.push(`offset ${p.offset}`)
  if (parts.length) {
    return `写入值是工程量，gatewayd 按 ${parts.join(' / ')} 反算寄存器原始值。`
  }
  return '写入值直接编码为寄存器原始值。'
})

let dispose: (() => void) | null = null

function resubscribe(): void {
  dispose?.()
  dispose = null
  const d = selected.value
  const pkg = selectedPkg.value
  if (!d || !pkg) return
  const list: string[] = []
  for (const pt of pkg.points) {
    list.push(`${d.id}.${pt.id}`)
    for (const b of pt.bits) list.push(`${d.id}.${pt.id}.b${b.bit}`)
  }
  dispose = tags.subscribe(list)
}

watch([selectedId, selectedPkg], resubscribe)

watch(
  [() => project.devices, () => props.deviceId, () => props.embedded ? undefined : route.query.device],
  ([list, deviceId, requested]) => {
    const id = deviceId || requested
    if (typeof id === 'string' && list.some((d) => d.id === id)) {
      selectedId.value = id
      return
    }
    if (!selectedId.value || !list.some((d) => d.id === selectedId.value)) {
      selectedId.value = list[0]?.id ?? ''
    }
  },
  { immediate: true },
)

function selectDevice(id: string): void {
  selectedId.value = id
  if (route.path === '/diagnostics') {
    void router.replace({ query: { ...route.query, device: id } })
  }
}

onBeforeUnmount(() => {
  dispose?.()
  dispose = null
  for (const t of flashTimers.values()) clearTimeout(t)
  flashTimers.clear()
})

function openWrite(row: Row): void {
  if (!project.live) return
  const cur = tags.get(row.tag)
  const p = primitiveOf(cur?.value)
  writeTarget.value = row
  writeError.value = ''
  if (isBitArea(row.point.area)) writeBool.value = p === true
  else writeNumber.value = typeof p === 'number' ? p : null
  writeOpen.value = true
}

async function submitWrite(): Promise<void> {
  const row = writeTarget.value
  if (!row || !project.live) return
  let value: TagValue
  if (isBoolArea(row.point)) {
    value = { bool: writeBool.value }
  } else {
    if (writeNumber.value === null || Number.isNaN(writeNumber.value)) {
      writeError.value = '请输入数值'
      return
    }
    const integer = !['f32', 'f64'].includes(row.point.type)
    const hasScale = row.point.scale !== null || row.point.offset !== null
    value = integer && !hasScale
      ? { i64: Math.round(writeNumber.value) }
      : { f64: writeNumber.value }
  }
  writing.value = true
  writeError.value = ''
  try {
    await gateway.request({ type: 'writeTag', tag: row.tag, value }, ['commandOk', 'error'])
    toast.success(`已写入 ${row.point.name || row.point.id}`)
    writeOpen.value = false
  } catch (e) {
    writeError.value = errorMessage(e)
  } finally {
    writing.value = false
  }
}

function isBoolArea(p: PointDto): boolean {
  return isBitArea(p.area)
}

function gotoDevices(): void {
  router.push('/devices')
}
</script>

<template>
  <div class="page tags-page" :class="{ embedded }">
    <PageHeader
      v-if="!embedded && !hideHeader"
      title="实时点表"
      subtitle="只订阅当前设备的点位；质量戳随每个值下发"
    >
      <span class="live-badge"><span class="live-dot" />LIVE</span>
    </PageHeader>

    <div v-if="!project.live" class="banner warn" role="status">
      <AppIcon name="alert" :size="16" />
      <span>{{ conn.connected ? '正在同步数据。' : '网关连接已断开，点表暂停更新。' }}缓存值旁显示末次采样时间。</span>
      <n-button v-if="!conn.connected" size="tiny" quaternary @click="router.push('/diagnostics')">连接诊断</n-button>
    </div>

    <div v-if="project.devices.length" class="tags-layout" :class="{ embedded }">
      <aside v-if="!embedded" class="card device-panel">
        <div class="panel-head">
          <h3>设备</h3>
          <span class="muted small">{{ project.live ? `${project.onlineDeviceCount}/${project.devices.length} 在线` : '状态待确认' }}</span>
        </div>
        <n-input v-model:value="deviceFilter" size="small" clearable placeholder="搜索设备" aria-label="搜索设备">
          <template #prefix><AppIcon name="search" :size="14" /></template>
        </n-input>
        <div class="device-list">
          <button
            v-for="d in filteredDevices"
            :key="d.id"
            type="button"
            class="device-item"
            :class="{ active: d.id === selectedId }"
            @click="selectDevice(d.id)"
          >
            <span class="online-dot" :class="{ on: project.live && d.online }" />
            <span class="device-item-main">
              <strong>{{ d.name || d.id }}</strong>
              <small class="mono">{{ d.id }} · {{ addressLabel(d.address) }}</small>
            </span>
            <span class="device-item-pkg">{{ project.packageOf(d)?.name ?? d.package }}</span>
          </button>
          <div v-if="filteredDevices.length === 0" class="muted small pad-12">没有匹配的设备</div>
        </div>
      </aside>

      <section v-if="selected" class="card tags-panel">
        <header class="tags-head">
          <div>
            <div class="row gap-8 align-center">
              <span class="online-dot" :class="{ on: project.live && selected.online }" />
              <h3>{{ selected.name || selected.id }}</h3>
              <span class="slave-badge mono">{{ addressLabel(selected.address) }}</span>
              <span class="muted small">{{ selectedChannel?.name || selected.channelId }} · {{ selectedChannel ? channelSummary(selectedChannel) : '—' }}</span>
            </div>
            <div class="tags-sub muted small">
              <template v-if="selectedPkg">{{ selectedPkg.name }} v{{ selectedPkg.version }} · {{ tagCount }} 个采集点</template>
              <template v-if="project.live">
                · <span class="text-good">{{ goodCount }} 良好</span>
                <span v-if="lostCount"> · <span class="text-bad">{{ lostCount }} 失联</span></span>
              </template>
              <span v-if="pendingCount"> · {{ pendingCount }} 行为缓存值</span>
            </div>
          </div>
          <n-button v-if="!embedded" size="tiny" quaternary @click="router.push({ path: '/devices' })">
            <template #icon><AppIcon name="chip" :size="14" /></template>
            设备管理
          </n-button>
        </header>

        <div v-if="selectedPkg" class="tags-toolbar">
          <div class="tags-search">
            <n-input v-model:value="pointFilter" size="small" clearable placeholder="搜索点位名称或 ID" aria-label="搜索点位">
              <template #prefix><AppIcon name="search" :size="14" /></template>
            </n-input>
          </div>
          <button type="button" class="filter-chip" :class="{ active: issuesOnly }" :aria-pressed="issuesOnly" @click="issuesOnly = !issuesOnly">
            只看异常
          </button>
          <span class="muted small">{{ filteredRows.length }} / {{ rows.length }} 个点位</span>
        </div>

        <div v-if="!selectedPkg" class="banner warn m-12">
          <AppIcon name="alert" :size="16" />
          <span>设备包 {{ selected.package }} 未安装，无法显示点位。</span>
        </div>

        <div v-else class="data-table tags-table">
          <div class="dt-head">
            <span>点位</span>
            <span>地址</span>
            <span>类型</span>
            <span>读数</span>
            <span>质量</span>
            <span>采样</span>
            <span class="align-right">操作</span>
          </div>
          <div v-for="row in filteredRows" :key="row.tag" class="dt-row">
            <div class="cell point-cell">
              <span class="point-name">{{ row.point.name || row.point.id }}</span>
              <code class="point-id">{{ row.point.id }}</code>
            </div>
            <div class="cell mono muted">{{ addressText(row.point.address) }}</div>
            <div class="cell">
              <span class="type-chip">{{ typeLabel(row.point) }}</span>
            </div>
            <div class="cell value-cell">
              <span
                class="value mono"
                :class="[qualityClass(row.quality), { flash: !row.stale && flashing.has(row.tag) }]"
              >{{ row.valueText }}</span>
              <span v-if="row.point.unit" class="unit">{{ row.point.unit }}</span>
              <div v-if="row.bits.length" class="bit-row">
                <span
                  v-for="b in row.bits"
                  :key="b.bit"
                  class="bit-chip"
                  :class="[{ on: b.value === true && !b.stale }, qualityClass(b.quality)]"
                  :title="`${b.name} · ${b.text}${b.stale ? ' · 缓存' : ''}`"
                >
                  <span class="bit-dot" />
                  {{ b.name }}
                  <em>{{ b.text }}</em>
                </span>
              </div>
            </div>
            <div class="cell"><QualityPill :quality="row.quality" :stale="row.stale" /></div>
            <div class="cell mono muted" :title="row.sampleAt ? new Date(row.sampleAt).toLocaleString('zh-CN') : undefined">
              {{ row.sampleAt ? (row.stale || row.quality === 'comm_lost' ? formatClock(row.sampleAt) : formatAge(nowMs - row.sampleAt)) : '—' }}
            </div>
            <div class="cell align-right">
              <n-button v-if="row.point.access === 'rw'" size="tiny" :disabled="!project.live" @click="openWrite(row)">写值</n-button>
              <span v-else class="muted small">只读</span>
            </div>
          </div>
          <div v-if="filteredRows.length === 0" class="dt-empty muted">没有匹配的点位</div>
        </div>
      </section>
    </div>

    <EmptyState
      v-else-if="project.loaded"
      icon="activity"
      title="还没有可展示的设备"
      text="先在「设备」页添加从站设备，实时点表会按设备包点表自动生成。"
    >
      <n-button size="small" type="primary" @click="gotoDevices">去添加设备</n-button>
    </EmptyState>

    <!-- 写值 -->
    <ModalShell
      v-if="writeOpen && writeTarget"
      :title="`写值 · ${writeTarget.point.name || writeTarget.point.id}`"
      :subtitle="writeTarget.tag"
      :width="480"
      @close="writeOpen = false"
    >
      <div class="write-summary">
        <div><span class="label">地址</span><span class="mono">{{ addressText(writeTarget.point.address) }}</span></div>
        <div><span class="label">类型</span><span>{{ typeLabel(writeTarget.point) }}</span></div>
        <div><span class="label">当前值</span><span class="mono">{{ writeTarget.valueText }} {{ writeTarget.point.unit }}</span></div>
      </div>

      <div class="field mt-12">
        <label>写入值{{ writeTarget.point.unit ? `（${writeTarget.point.unit}）` : '' }}</label>
        <n-switch v-if="isBoolPoint" v-model:value="writeBool" size="large">
          <template #checked>ON</template>
          <template #unchecked>OFF</template>
        </n-switch>
        <n-input-number
          v-else
          v-model:value="writeNumber"
          :show-button="false"
          placeholder="输入工程量数值"
          class="w-full"
        />
        <p class="hint">{{ writeHint }}</p>
      </div>

      <div v-if="writeError" class="banner danger mt-8">
        <AppIcon name="alert" :size="16" />
        <span>{{ writeError }}</span>
      </div>

      <template #footer>
        <n-button size="small" @click="writeOpen = false">取消</n-button>
        <n-button size="small" type="primary" :loading="writing" @click="submitWrite">写入</n-button>
      </template>
    </ModalShell>
  </div>
</template>
