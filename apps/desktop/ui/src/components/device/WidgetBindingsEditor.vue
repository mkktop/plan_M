<script setup lang="ts">
import { computed } from 'vue'
import { NSelect } from 'naive-ui'
import { bindingDecls, HOST_DEVICE_COMPONENTS } from '@/utils/devicePages'
import type { BindingDecl, BindingSpec, PointDto, WidgetBindingDto } from '@/api/types'

type BindingPoint = Pick<PointDto, 'id' | 'name'> & { bits: Pick<PointDto['bits'][number], 'bit' | 'name'>[] }

const props = defineProps<{
  modelValue: string
  kind: string
  points: BindingPoint[]
  declarations?: Record<string, WidgetBindingDto>
  devices?: { id: string; name: string; points: BindingPoint[] }[]
}>()
const emit = defineEmits<{ 'update:modelValue': [value: string] }>()

const slots = computed(() => {
  const key = HOST_DEVICE_COMPONENTS.find(({ kind }) => kind === props.kind)?.slot
  const bindings = props.declarations ?? (key ? { [key]: { type: 'point', many: key === 'values' } } : {})
  return Object.entries(bindings).filter(([, info]) => info.type !== 'text')
})

const parsed = computed<Record<string, BindingSpec> | null>(() => {
  if (!props.modelValue.trim()) return {}
  try {
    const value = JSON.parse(props.modelValue)
    return value && typeof value === 'object' && !Array.isArray(value) ? value : null
  } catch {
    return null
  }
})

function optionsFor(points: BindingPoint[], device?: { id: string; name: string }) {
  return points.flatMap((point) => {
  const name = `${device ? `${device.name || device.id} · ` : ''}${point.name || point.id}`
  const binding = (bit?: number) => JSON.stringify(device
    ? { tag: `${device.id}.${point.id}${bit === undefined ? '' : `.b${bit}`}` }
    : { point: point.id, bit })
  const base = { label: `${name} · ${point.id}`, value: binding() }
  return props.kind === 'core:bits' ? [base] : [
    base,
    ...point.bits.map((bit) => ({
      label: `${name} / ${bit.name}`,
      value: binding(bit.bit),
    })),
  ]
  })
}

const pointOptions = computed(() => [
  ...optionsFor(props.points),
  ...(props.devices ?? []).flatMap((device) => optionsFor(device.points, device)),
])

function keys(slot: string): string[] {
  return bindingDecls(parsed.value?.[slot]).flatMap((binding) => {
    if (typeof binding?.point === 'string' && binding.point) {
      return [JSON.stringify({ point: binding.point, bit: binding.bit ?? undefined })]
    }
    return typeof binding?.tag === 'string' && binding.tag ? [JSON.stringify({ tag: binding.tag })] : []
  })
}

function choices(slot: string) {
  const options = pointOptions.value
  const extras = keys(slot).filter((value) => !options.some((option) => option.value === value))
  return [...options, ...extras.map((value) => {
    const { point, tag, bit } = JSON.parse(value) as BindingDecl
    return { label: tag ?? (bit == null ? (point ?? value) : `${point}.b${bit}`), value }
  })]
}

function updateSlot(slot: string, value: string | string[] | null): void {
  if (!parsed.value) return
  const next = { ...parsed.value }
  if (!value || !value.length) {
    delete next[slot]
  } else {
    next[slot] = Array.isArray(value) ? value.map((item) => JSON.parse(item)) : JSON.parse(value)
  }
  emit('update:modelValue', Object.keys(next).length ? JSON.stringify(next, null, 2) : '')
}
</script>

<template>
  <div v-if="parsed && slots.length" class="widget-binding-fields">
    <label v-for="[slot, info] in slots" :key="slot" class="field">
      <span>{{ slot === 'value' ? '绑定点位' : slot === 'values' ? '绑定多个点位' : `绑定槽位 ${slot}` }}</span>
      <n-select
        :options="choices(slot)"
        :value="info.many ? keys(slot) : (keys(slot)[0] ?? null)"
        :multiple="Boolean(info.many)"
        filterable
        clearable
        size="small"
        placeholder="选择点位或状态位"
        @update:value="(value: string | string[] | null) => updateSlot(slot, value)"
      />
    </label>
  </div>
</template>
