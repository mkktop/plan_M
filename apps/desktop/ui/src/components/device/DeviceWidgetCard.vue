<script setup lang="ts">
//! 设备页面的内置小组件（core:value / gauge / switch / input / bits）。
//!
//! 组件只认槽位绑定：`bind.value` 由宿主解析成完整 tag（相对点 + 当前设备实例），
//! 数据来自 tags store。配置（config）由组件自行解释，宿主原样透传。
//!
//! 订阅按可见性门控（IntersectionObserver，提前 200px），卸载即退订；
//! 同一 tag 的重复订阅由 gateway 客户端按引用计数合并。

import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { NButton, NInputNumber, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import QualityPill from '@/components/QualityPill.vue'
import ExternalWidgetMount from '@/components/device/ExternalWidgetMount.vue'
import GroupBody from '@/components/device/GroupBody.vue'
import LiveStatistics from '@/components/device/LiveStatistics.vue'
import LiveTrend from '@/components/device/LiveTrend.vue'
import { gateway } from '@/api/ws'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { useTagsStore } from '@/stores/tags'
import { errorMessage, useToastStore } from '@/stores/toast'
import { resolveExternalWidget } from '@/plugins/deviceWidgets'
import {
  componentLabel,
  isExternalWidgetKind,
  normalizeComponentKind,
  resolveBoundTag,
  resolveBoundTags,
  slotBind,
} from '@/utils/devicePages'
import { boolOf, formatNumber, primitiveOf, qualityClass, valueText } from '@/utils/format'
import { effectivePoint, numericTagValue } from '@/utils/devicePoints'
import type {
  BitDto,
  DevicePageComponentDto,
  PointDto,
  TagValue,
} from '@/api/types'

const props = defineProps<{
  deviceId: string
  component: DevicePageComponentDto
}>()

const project = useProjectStore()
const connection = useConnectionStore()
const tags = useTagsStore()
const toast = useToastStore()

const kind = computed(() => normalizeComponentKind(props.component.kind))
const config = computed<Record<string, unknown>>(() => props.component.config ?? {})
const binding = computed(() => slotBind(props.component.bind, 'value'))

/** 当前设备对应的设备包（外部组件的点表元数据也按它解析）。 */
const pkg = computed(() => {
  const device = project.devices.find((d) => d.id === props.deviceId)
  return device ? (project.packageById.get(device.package) ?? null) : null
})

/** 插件 / 设备包 ESM 组件：交给统一宿主挂载。 */
const external = computed(() =>
  isExternalWidgetKind(props.component.kind)
    ? resolveExternalWidget(props.component.kind, project.packageById.get(props.component.kind.split(':')[0]) ?? pkg.value)
    : null,
)
const externalMissing = computed(
  () => isExternalWidgetKind(props.component.kind) && !external.value,
)
const externalMeta = computed(() =>
  external.value ? `${external.value.providerName} · ${external.value.kind}` : '',
)

function cfgNumber(key: string, fallback: number): number {
  const v = config.value[key]
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback
}

function cfgString(key: string, fallback: string): string {
  const v = config.value[key]
  return typeof v === 'string' ? v : fallback
}

// —— 绑定解析 ——

const tag = computed(() => resolveBoundTag(props.deviceId, binding.value))
const readingTags = computed(() => [...new Set(resolveBoundTags(props.deviceId, slotBind(props.component.bind, 'values')))])
const unbound = computed(() => {
  if (external.value || externalMissing.value || kind.value === 'core:group' || kind.value === 'core:text') return false
  return kind.value === 'core:readings' ? !readingTags.value.length : !tag.value
})
/** 位绑定写值要写父点（gateway 的位子 tag 只读）。 */
const parentTag = computed(() => tag.value.replace(/\.b\d+$/, ''))

interface Resolved {
  point: PointDto
  bit?: BitDto
}

function resolvePoint(fullTag: string): Resolved | null {
  if (!fullTag) return null
  const [deviceId, pointId, bitId] = fullTag.split('.')
  const device = project.devices.find((d) => d.id === deviceId)
  const pkg = device ? project.packageById.get(device.package) : null
  const point = pkg?.points.find((p) => p.id === pointId)
  if (!point) return null
  const bit =
    bitId && bitId.startsWith('b')
      ? point.bits.find((b) => `b${b.bit}` === bitId)
      : undefined
  if (bitId && !bit) return null
  return { point: effectivePoint(device, point), bit }
}

const resolved = computed(() => resolvePoint(tag.value))
const bitIndex = computed(() => resolved.value?.bit?.bit)
const bitBound = computed(() => bitIndex.value !== undefined)

function changeOf(t: string) {
  return t ? tags.getLive(t) : undefined
}

/** 开关绑位时订阅父点（读整字、写整字）；其余订阅目标 tag。 */
const primaryTag = computed(() => (kind.value === 'core:switch' && bitBound.value ? parentTag.value : tag.value))

const change = computed(() => changeOf(primaryTag.value))
const quality = computed(() => change.value?.quality)

const displayTitle = computed(() => {
  const own = props.component.title.trim()
  if (own) return own
  if (external.value) return external.value.title
  if (externalMissing.value) return '未知组件'
  if (!tag.value) return componentLabel(kind.value)
  const r = resolved.value
  if (!r) return tag.value
  return r.bit ? `${r.point.name} · ${r.bit.name}` : r.point.name
})

const unit = computed(() => cfgString('unit', resolved.value?.point?.unit ?? ''))

const min = computed(() => {
  const meta = resolved.value?.point?.min
  return cfgNumber('min', typeof meta === 'number' ? meta : 0)
})

const max = computed(() => {
  const meta = resolved.value?.point?.max
  return cfgNumber('max', typeof meta === 'number' ? meta : 100)
})

const decimals = computed(() => {
  const n = cfgNumber('decimals', cfgNumber('precision', 1))
  return Math.max(0, Math.min(6, Math.round(n)))
})

const readings = computed(() => {
  if (kind.value !== 'core:readings') return []
  return readingTags.value.map((tag) => {
    const r = resolvePoint(tag)
    const status = tags.getLive(tag)
    const bitValue = boolOf(status?.value)
    const title = r?.bit ? `${r.point.name} · ${r.bit.name}` : r?.point.name || tag
    const value = r?.bit && bitValue !== undefined
      ? (bitValue ? (r.bit.on ?? '1') : (r.bit.off ?? '0'))
      : valueText(status?.value)
    return { tag, title, value, unit: r?.bit ? '' : r?.point.unit, quality: status?.quality }
  })
})

const indicatorOn = computed(() =>
  connection.connected && quality.value === 'good' ? boolOf(change.value?.value) : undefined,
)
const indicatorText = computed(() => {
  if (indicatorOn.value === undefined) return '等待有效状态'
  return indicatorOn.value
    ? cfgString('onLabel', resolved.value?.bit?.on ?? '开启')
    : cfgString('offLabel', resolved.value?.bit?.off ?? '关闭')
})

const thresholdState = computed(() => {
  if (max.value <= min.value) return { tone: 'warn', text: '上限需大于下限' }
  const current = primitiveOf(change.value?.value)
  if (!connection.connected || quality.value !== 'good' || typeof current !== 'number' || !Number.isFinite(current)) {
    return { tone: 'idle', text: '等待有效读数' }
  }
  if (current < min.value) return { tone: 'warn', text: '低于下限' }
  if (current > max.value) return { tone: 'bad', text: '高于上限' }
  return { tone: 'ok', text: '在范围内' }
})

const displayValue = computed(() => {
  const v = change.value?.value
  if (!v) return '—'
  const r = resolved.value
  // 位绑定：显示位定义文案（开关除外，开关自己渲染 on/off 标签）
  if (r?.bit && kind.value !== 'core:switch') {
    const on = boolOf(v) ?? false
    return on ? (r.bit.on ?? '1') : (r.bit.off ?? '0')
  }
  const p = primitiveOf(v)
  if (typeof p === 'number') return formatNumber(p, decimals.value)
  return valueText(v)
})

// 数值变化闪动：新值到达时短暂加 .flash（600ms）
const flashing = ref(false)
let flashTimer: ReturnType<typeof setTimeout> | null = null
watch(displayValue, (cur, prev) => {
  if (cur === prev || cur === '—') return
  flashing.value = true
  if (flashTimer) clearTimeout(flashTimer)
  flashTimer = setTimeout(() => {
    flashing.value = false
    flashTimer = null
  }, 600)
})

const gaugePct = computed(() => {
  const p = primitiveOf(change.value?.value)
  if (typeof p !== 'number') return 0
  if (max.value === min.value) return 0
  return Math.max(0, Math.min(100, ((p - min.value) / (max.value - min.value)) * 100))
})

const levelReady = computed(() => {
  const value = primitiveOf(change.value?.value)
  return connection.connected && quality.value === 'good' &&
    typeof value === 'number' && Number.isFinite(value) && max.value > min.value
})

/** 弧线端点（gauge-fill 的圆头位置）：半圆圆心 (100,100)、半径 80，180°→0°。 */
const gaugeTipPos = computed(() => {
  const theta = Math.PI * (1 - gaugePct.value / 100)
  return {
    x: Number((100 + 80 * Math.cos(theta)).toFixed(2)),
    y: Number((100 - 80 * Math.sin(theta)).toFixed(2)),
  }
})

const metaText = computed(() => {
  if (external.value) return externalMeta.value
  if (externalMissing.value) return '组件来源未安装或已移除'
  if (kind.value === 'core:readings') return readings.value.length ? `${readings.value.length} 个点位` : ''
  if (kind.value === 'core:text' || kind.value === 'core:group') return ''
  if (!tag.value) return props.component.title || '在页面编辑里填写 bind.value'
  const r = resolved.value
  if (!r) return '点不在当前点表中'
  const bit = r.bit ? ` · ${r.bit.name}` : ''
  return `${r.point.name}${bit} · ${tag.value}`
})

// —— 位表（core:bits）——

interface BitRow {
  def: BitDto
  tag: string
  on: boolean
  quality?: ReturnType<typeof qualityOf>
}

function qualityOf(t: string) {
  return tags.getLive(t)?.quality
}

const bitRows = computed<BitRow[]>(() => {
  const r = resolved.value
  if (kind.value !== 'core:bits' || !r || r.bit) return []
  return r.point.bits.map((def) => {
    const t = `${parentTag.value}.b${def.bit}`
    return { def, tag: t, on: boolOf(tags.getLive(t)?.value) ?? false, quality: qualityOf(t) }
  })
})

// —— 写值 ——

const writing = ref(false)
const writable = computed(() => {
  const r = resolved.value
  if (!r || r.point.access !== 'rw') return false
  if (kind.value === 'core:input') return !bitBound.value
  if (kind.value === 'core:switch') return true
  return false
})

const switchOn = computed(() => {
  if (!bitBound.value) return boolOf(change.value?.value) ?? false
  const num = primitiveOf(change.value?.value)
  if (typeof num !== 'number') return false
  return ((num >> (bitIndex.value ?? 0)) & 1) === 1
})

const switchOnLabel = computed(() => {
  const r = resolved.value
  return cfgString('onLabel', r?.bit?.on ?? 'ON')
})

const switchOffLabel = computed(() => {
  const r = resolved.value
  return cfgString('offLabel', r?.bit?.off ?? 'OFF')
})


async function writeTag(target: string, value: TagValue, okText: string): Promise<boolean> {
  if (writing.value) return false
  writing.value = true
  try {
    await gateway.request({ type: 'writeTag', tag: target, value }, ['commandOk', 'error'], 8000)
    toast.success(okText)
    return true
  } catch (e) {
    toast.error(`写入失败：${errorMessage(e)}`)
    return false
  } finally {
    writing.value = false
  }
}

async function toggleSwitch(): Promise<void> {
  const r = resolved.value
  if (!r || !writable.value) return
  const on = switchOn.value
  if (bitBound.value) {
    const num = primitiveOf(change.value?.value)
    if (typeof num !== 'number' || change.value?.quality !== 'good') {
      toast.error('还没有读到父点当前值，稍后再试')
      return
    }
    // 位写 = 读父点整字 → 改位 → 写回；带工程量换算时 tag 值不是原始位域，拒绝以免写错。
    if ((r.point.scale != null && r.point.scale !== 1) || (r.point.offset != null && r.point.offset !== 0)) {
      toast.error('该点带 scale/offset 换算，位写入请直接写父点')
      return
    }
    const bit = bitIndex.value ?? 0
    const next = on ? num & ~(1 << bit) : num | (1 << bit)
    await writeTag(parentTag.value, numericTagValue(r.point, next), `已写入 ${displayTitle.value}`)
    return
  }
  const value: TagValue =
    r.point.area === 'coil' ? { bool: !on } : { i64: on ? 0 : 1 }
  await writeTag(tag.value, value, `已写入 ${displayTitle.value}`)
}

// —— 输入组件（core:input）——

const enumOptions = computed(() =>
  (resolved.value?.point.enum ?? []).map((e) => ({
    label: e.label || String(e.value),
    value: e.value,
  })),
)
const isEnum = computed(() => enumOptions.value.length > 0)

const inputValue = ref<number | null>(null)
const enumValue = ref<number | null>(null)
const dirty = ref(false)

const coilPoint = computed(
  () => resolved.value?.point.area === 'coil' || resolved.value?.point.area === 'discrete',
)

const inputMin = computed<number | undefined>(() => {
  if (config.value.min !== undefined) return cfgNumber('min', 0)
  if (coilPoint.value) return 0
  const m = resolved.value?.point.min
  return typeof m === 'number' ? m : undefined
})
const inputMax = computed<number | undefined>(() => {
  if (config.value.max !== undefined) return cfgNumber('max', 0)
  if (coilPoint.value) return 1
  const m = resolved.value?.point.max
  return typeof m === 'number' ? m : undefined
})
const inputStep = computed<number | undefined>(() => {
  if (config.value.step !== undefined) return cfgNumber('step', 1)
  if (coilPoint.value) return 1
  const s = resolved.value?.point.step
  return typeof s === 'number' ? s : undefined
})
const inputPrecision = computed(() => {
  if (config.value.decimals !== undefined) return decimals.value
  const step = inputStep.value
  if (step === undefined) return 0
  if (step >= 1) return 0
  return Math.min(6, Math.max(0, Math.ceil(-Math.log10(step))))
})

watch(
  change,
  (ch) => {
    if (dirty.value) return
    const p = primitiveOf(ch?.value)
    if (typeof p === 'number') {
      inputValue.value = p
      enumValue.value = p
    }
  },
  { immediate: true },
)

function onInput(v: number | null): void {
  inputValue.value = v
  dirty.value = true
}

function onEnum(v: number | null): void {
  enumValue.value = v
  dirty.value = true
}

async function applyInput(): Promise<void> {
  const r = resolved.value
  if (!r || !writable.value) return
  const v = isEnum.value ? enumValue.value : inputValue.value
  if (v === null || !Number.isFinite(v)) {
    toast.error('请输入有效数值')
    return
  }
  if (inputMin.value !== undefined && v < inputMin.value) {
    toast.error(`写入值不能小于 ${inputMin.value}`)
    return
  }
  if (inputMax.value !== undefined && v > inputMax.value) {
    toast.error(`写入值不能大于 ${inputMax.value}`)
    return
  }
  const ok = await writeTag(tag.value, numericTagValue(r.point, v), `已写入 ${displayTitle.value}`)
  if (ok) dirty.value = false
}

// —— 订阅：按可见性与 tag 集合重建，卸载即退订 ——

const rootEl = ref<HTMLElement | null>(null)
const visible = ref(true)
let io: IntersectionObserver | null = null

const tagsToWatch = computed<string[]>(() => {
  // 外部组件自己通过 host.slots 订阅，这里不重复订阅。
  if (external.value || externalMissing.value) return []
  if (kind.value === 'core:readings') return readingTags.value
  if (kind.value === 'core:bits') {
    const r = resolved.value
    if (!r || r.bit) return []
    return r.point.bits.map((b) => `${parentTag.value}.b${b.bit}`)
  }
  return primaryTag.value ? [primaryTag.value] : []
})

let dispose: (() => void) | null = null

function resubscribe(): void {
  dispose?.()
  dispose = null
  if (!visible.value) return
  const list = [...new Set(tagsToWatch.value)]
  if (list.length) dispose = tags.subscribe(list)
}

watch([visible, () => tagsToWatch.value.join('|')], resubscribe, { immediate: true })

onMounted(() => {
  if (typeof IntersectionObserver === 'undefined' || !rootEl.value) return
  io = new IntersectionObserver(
    (entries) => {
      visible.value = entries[0]?.isIntersecting ?? true
    },
    { rootMargin: '200px' },
  )
  io.observe(rootEl.value)
})

onBeforeUnmount(() => {
  io?.disconnect()
  io = null
  dispose?.()
  dispose = null
  if (flashTimer) clearTimeout(flashTimer)
})
</script>

<template>
  <article
    ref="rootEl"
    class="dev-widget"
    :class="[
      `kind-${kind.replace(':', '-')}`,
      { unbound, external: Boolean(external) },
    ]"
  >
    <header class="dev-widget-head">
      <span class="dev-widget-title" :title="displayTitle">{{ displayTitle }}</span>
      <span v-if="metaText" class="dev-widget-meta" :title="metaText">{{ metaText }}</span>
    </header>

    <ExternalWidgetMount
      v-if="external"
      :descriptor="external"
      :component="component"
      :device-id="deviceId"
      :pkg="pkg"
    />

    <div v-else-if="externalMissing" class="dev-widget-empty">
      <AppIcon name="alert" :size="15" />
      <span>未知组件 <code>{{ component.kind }}</code>（来源未安装或已移除）</span>
    </div>

    <div v-else-if="unbound" class="dev-widget-empty">
      <AppIcon name="alert" :size="15" />
      <span>未绑定数据点</span>
    </div>

    <div v-else-if="kind === 'core:text'" class="widget-message" :class="{ emphasized: config.tone === 'accent' }">
      {{ cfgString('text', '') }}
    </div>

    <div v-else-if="kind === 'core:readings'" class="widget-readings">
      <div v-for="reading in readings" :key="reading.tag" class="reading-row">
        <span class="reading-title" :title="reading.tag">{{ reading.title }}</span>
        <span class="reading-value mono">{{ reading.value }}<small v-if="reading.unit"> {{ reading.unit }}</small></span>
        <QualityPill :quality="reading.quality" compact />
      </div>
    </div>

    <div v-else-if="kind === 'core:indicator'" class="dev-widget-body">
      <div class="indicator-content">
        <span class="indicator-lamp" :class="{ lit: indicatorOn === true, unknown: indicatorOn === undefined }">
          <AppIcon name="lamp" :size="21" />
        </span>
        <strong>{{ indicatorText }}</strong>
      </div>
      <div class="value-foot"><QualityPill :quality="connection.connected ? quality : undefined" /></div>
    </div>

    <div v-else-if="kind === 'core:threshold'" class="dev-widget-body">
      <div class="threshold-readout" :class="thresholdState.tone">
        <span class="trend-number mono">{{ connection.connected && quality === 'good' ? displayValue : '—' }}</span>
        <span v-if="unit" class="value-unit">{{ unit }}</span>
        <span class="state-pill" :class="thresholdState.tone">{{ thresholdState.text }}</span>
      </div>
      <div class="range-scale mono">
        <span>下限 {{ formatNumber(min, decimals) }}</span>
        <span>上限 {{ formatNumber(max, decimals) }}</span>
      </div>
    </div>

    <div v-else-if="kind === 'core:stats'" class="dev-widget-body">
      <LiveStatistics :change="change" :connected="connection.connected" :decimals="decimals" :unit="unit" />
    </div>

    <template v-else-if="kind === 'core:value'">
      <div class="dev-widget-body">
        <div class="value-main">
          <span class="value-number mono" :class="[qualityClass(quality), { flash: flashing }]">{{ displayValue }}</span>
          <span v-if="unit" class="value-unit">{{ unit }}</span>
        </div>
        <div class="value-foot">
          <QualityPill compact :quality="quality" />
          <span v-if="!change" class="muted small">等待数据</span>
        </div>
      </div>
    </template>

    <template v-else-if="kind === 'core:gauge'">
      <div class="dev-widget-body gauge-body">
        <div class="gauge-wrap">
          <svg class="gauge-svg" viewBox="0 0 200 108" preserveAspectRatio="xMidYMid meet">
            <path class="gauge-track" d="M20 100 A80 80 0 0 1 180 100" pathLength="100" />
            <path
              class="gauge-fill"
              :class="qualityClass(quality)"
              d="M20 100 A80 80 0 0 1 180 100"
              pathLength="100"
              :stroke-dasharray="`${gaugePct} 100`"
            />
            <circle
              v-if="change"
              class="gauge-tip"
              :class="qualityClass(quality)"
              :cx="gaugeTipPos.x"
              :cy="gaugeTipPos.y"
              r="4.5"
            />
          </svg>
          <div class="gauge-readout">
            <span class="gauge-value mono" :class="[qualityClass(quality), { flash: flashing }]">{{ displayValue }}</span>
            <span v-if="unit" class="gauge-unit">{{ unit }}</span>
          </div>
          <div class="gauge-range">
            <span>{{ formatNumber(min, 2) }}</span>
            <span>{{ formatNumber(max, 2) }}</span>
          </div>
        </div>
        <div class="value-foot">
          <QualityPill compact :quality="quality" />
        </div>
      </div>
    </template>

    <template v-else-if="kind === 'core:trend'">
      <div class="dev-widget-body">
        <div class="value-main">
          <span class="trend-number mono">{{ connection.connected && quality === 'good' ? displayValue : '—' }}</span>
          <span v-if="unit" class="value-unit">{{ unit }}</span>
        </div>
        <LiveTrend :key="tag" :change="change" :connected="connection.connected" :decimals="decimals" />
        <div class="value-foot"><QualityPill compact :quality="connection.connected ? quality : undefined" /></div>
      </div>
    </template>

    <template v-else-if="kind === 'core:level'">
      <div class="dev-widget-body range-widget">
        <div class="value-main">
          <span class="trend-number mono">{{ levelReady ? displayValue : '—' }}</span>
          <span v-if="unit" class="value-unit">{{ unit }}</span>
        </div>
        <div class="range-track" :class="{ inactive: !levelReady }">
          <meter v-if="levelReady" :aria-label="displayTitle" min="0" max="100" :value="gaugePct" />
        </div>
        <div class="range-scale mono">
          <span>{{ formatNumber(min, decimals) }}</span>
          <span>{{ formatNumber(max, decimals) }}</span>
        </div>
        <div class="value-foot">
          <QualityPill compact :quality="connection.connected ? quality : undefined" />
          <span v-if="max <= min" class="text-warn small">量程上限需大于下限</span>
          <span v-else-if="levelReady" class="muted small">{{ formatNumber(gaugePct, 1) }}% 量程</span>
          <span v-else class="muted small">{{ connection.connected ? '等待有效数值' : '等待网关连接' }}</span>
        </div>
      </div>
    </template>

    <template v-else-if="kind === 'core:switch'">
      <div class="dev-widget-body">
        <div class="switch-main">
          <button
            type="button"
            class="switch-toggle"
            :class="{ on: switchOn, readonly: !writable, writing }"
            :disabled="!writable || writing"
            :title="writable ? '点击切换并写入' : '只读点或父点不可写'"
            @click="toggleSwitch"
          >
            <span class="switch-knob" />
          </button>
          <div class="switch-labels">
            <strong>{{ switchOn ? switchOnLabel : switchOffLabel }}</strong>
            <span class="muted small">
              {{ writing ? '写入中…' : writable ? '可写点' : '只读' }}
            </span>
          </div>
        </div>
        <div class="value-foot">
          <QualityPill compact :quality="quality" />
          <span v-if="!change" class="muted small">等待数据</span>
        </div>
      </div>
    </template>

    <template v-else-if="kind === 'core:input'">
      <div class="dev-widget-body dev-input-body">
        <div class="dev-input-row">
          <n-select
            v-if="isEnum"
            :value="enumValue"
            :options="enumOptions"
            size="small"
            :disabled="!writable || writing"
            @update:value="onEnum"
          />
          <n-input-number
            v-else
            :value="inputValue"
            size="small"
            :min="inputMin"
            :max="inputMax"
            :step="inputStep"
            :precision="inputPrecision"
            :disabled="!writable || writing"
            @update:value="onInput"
          />
          <n-button
            size="small"
            type="primary"
            :disabled="!writable || writing || (isEnum && enumValue === null)"
            @click="applyInput"
          >
            写入
          </n-button>
        </div>
        <div class="dev-input-hint muted small">
          <template v-if="!writable">只读点</template>
          <template v-else-if="isEnum">可选：{{ enumOptions.map((o) => o.label).join(' / ') }}</template>
          <template v-else>
            范围 {{ inputMin ?? '—' }} ~ {{ inputMax ?? '—' }}
            <template v-if="inputStep !== undefined"> · 步长 {{ inputStep }}</template>
          </template>
          <span v-if="dirty" class="dev-dirty"> · 未写入</span>
        </div>
      </div>
    </template>

    <template v-else-if="kind === 'core:bits'">
      <div class="dev-widget-body bits-body">
        <div v-if="!bitRows.length" class="dev-widget-empty">
          <AppIcon name="alert" :size="15" />
          <span>{{ resolved ? '该点没有 bits 定义' : '点不在当前点表中' }}</span>
        </div>
        <div v-else class="bits-list">
          <div v-for="row in bitRows" :key="row.def.bit" class="bits-row">
            <span class="bits-dot" :class="[qualityClass(row.quality), { on: row.on }]" />
            <span class="bits-name" :title="row.def.name">{{ row.def.name }}</span>
            <span class="bits-state" :class="{ on: row.on }">
              {{ row.on ? (row.def.on ?? '1') : (row.def.off ?? '0') }}
            </span>
          </div>
        </div>
      </div>
    </template>

    <template v-else-if="kind === 'core:group'">
      <div class="dev-widget-body dev-group-body">
        <GroupBody :device-id="deviceId" :component="component" />
      </div>
    </template>

    <div v-else class="dev-widget-empty">
      <AppIcon name="alert" :size="15" />
      <span>未知组件 <code>{{ component.kind }}</code></span>
    </div>
  </article>
</template>
