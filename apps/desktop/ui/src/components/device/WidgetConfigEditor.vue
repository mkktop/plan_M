<script setup lang="ts">
import { computed } from 'vue'
import { NInput, NInputNumber, NSelect, NSwitch } from 'naive-ui'
import { effectiveWidgetConfig } from '@/utils/widgetConfig'
import type { WidgetConfigSchema } from '@/api/types'

const props = defineProps<{ kind: string; config: Record<string, unknown>; schema?: WidgetConfigSchema; defaults?: Record<string, unknown> | null }>()
const emit = defineEmits<{ change: [config: Record<string, unknown>] }>()

const withFormat = computed(() => ['core:value', 'core:gauge', 'core:trend', 'core:level', 'core:threshold', 'core:stats'].includes(props.kind))
const withRange = computed(() => ['core:gauge', 'core:level', 'core:threshold'].includes(props.kind))
const withLabels = computed(() => ['core:switch', 'core:indicator'].includes(props.kind))
const tones = [{ label: '常规', value: 'plain' }, { label: '强调', value: 'accent' }]
const fields = computed(() => Object.entries(props.schema ?? {}))
const values = computed(() => effectiveWidgetConfig(props.schema, props.defaults ?? null, props.config))

function stringValue(key: string): string {
  const value = values.value[key]
  return typeof value === 'string' ? value : ''
}

function numberValue(key: string): number | null {
  const value = values.value[key]
  return typeof value === 'number' ? value : null
}

function set(key: string, value: string | number | boolean | null): void {
  const next = { ...props.config }
  if (value == null || (value === '' && !props.schema?.[key])) delete next[key]
  else next[key] = value
  emit('change', next)
}
</script>

<template>
  <div v-if="fields.length" class="widget-config-fields">
    <div v-for="[key, field] in fields" :key="key" class="field">
      <span :title="field.description">{{ field.label || key }}</span>
      <n-select v-if="field.type === 'select'" :value="stringValue(key) || null" size="small" clearable
        :options="field.choices?.map((choice) => ({ value: choice, label: choice }))" @update:value="set(key, $event)" />
      <n-switch v-else-if="field.type === 'boolean'" :value="values[key] === true" :aria-label="field.label" @update:value="set(key, $event)" />
      <n-input v-else-if="field.type === 'string'" :value="stringValue(key)" :aria-label="field.label"
        size="small" @update:value="set(key, $event)" />
      <n-input-number v-else :value="numberValue(key)" :min="field.min ?? undefined" :max="field.max ?? undefined"
        :input-props="{ 'aria-label': field.label }" :precision="field.type === 'integer' ? 0 : undefined"
        size="small" @update:value="set(key, $event)" />
      <button v-if="Object.hasOwn(config, key)" type="button" class="widget-field-reset" @click="set(key, null)">恢复默认</button>
    </div>
  </div>
  <div v-if="withFormat || withRange || withLabels || kind === 'core:text'" class="widget-config-fields">
    <template v-if="withFormat">
      <label class="field">
        <span>单位</span>
        <n-input :value="stringValue('unit')" placeholder="沿用点表单位" size="small" @update:value="set('unit', $event)" />
      </label>
      <label class="field">
        <span>小数位</span>
        <n-input-number :value="numberValue('decimals')" :min="0" :max="6" size="small" @update:value="set('decimals', $event)" />
      </label>
    </template>
    <template v-if="withRange">
      <label class="field">
        <span>下限</span>
        <n-input-number :value="numberValue('min')" size="small" @update:value="set('min', $event)" />
      </label>
      <label class="field">
        <span>上限</span>
        <n-input-number :value="numberValue('max')" size="small" @update:value="set('max', $event)" />
      </label>
    </template>
    <template v-if="withLabels">
      <label class="field">
        <span>开启时显示</span>
        <n-input :value="stringValue('onLabel')" size="small" @update:value="set('onLabel', $event)" />
      </label>
      <label class="field">
        <span>关闭时显示</span>
        <n-input :value="stringValue('offLabel')" size="small" @update:value="set('offLabel', $event)" />
      </label>
    </template>
    <template v-if="kind === 'core:text'">
      <label class="field widget-note-field">
        <span>说明内容</span>
        <n-input :value="stringValue('text')" type="textarea" :autosize="{ minRows: 2, maxRows: 5 }"
          placeholder="输入操作说明或分区介绍" size="small" @update:value="set('text', $event)" />
      </label>
      <label class="field">
        <span>样式</span>
        <n-select :value="stringValue('tone') || 'plain'" :options="tones" size="small" @update:value="set('tone', $event)" />
      </label>
    </template>
  </div>
</template>

<style scoped>
.widget-field-reset {
  align-self: start;
  padding: 1px 0;
  border: 0;
  background: none;
  color: var(--accent-text, var(--text-2));
  font: inherit;
  font-size: 11px;
  cursor: pointer;
}
</style>
