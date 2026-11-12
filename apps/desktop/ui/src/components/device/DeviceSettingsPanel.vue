<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { NButton, NInput, NInputNumber, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import EmptyState from '@/components/EmptyState.vue'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { errorMessage, useToastStore } from '@/stores/toast'
import { channelSummary, packageSupportsTransport } from '@/utils/transport'
import type { DeviceAddress, DeviceConfig } from '@/api/types'

const props = defineProps<{ deviceId: string }>()

const project = useProjectStore()
const conn = useConnectionStore()
const toast = useToastStore()

const device = computed(() => project.devices.find((d) => d.id === props.deviceId) ?? null)
const pkg = computed(() => project.packageOf(device.value))
const pointCount = computed(() =>
  pkg.value ? pkg.value.points.reduce((n, p) => n + 1 + p.bits.length, 0) : 0,
)

const channelOptions = computed(() => {
  const p = pkg.value
  return project.channels
    .filter(
      (c) =>
        !p ||
        c.id === form.channelId ||
        packageSupportsTransport(p, c.transport, project.drivers),
    )
    .map((c) => ({
      label: `${c.name || c.id}（${channelSummary(c)}）`,
      value: c.id,
    }))
})

const form = reactive({
  name: '',
  channelId: '',
  address: '1',
  overridesText: '{}',
})
const saving = ref(false)
const error = ref('')

function fillFromDevice(): void {
  const d = device.value
  if (!d) return
  form.name = d.name
  form.channelId = d.channelId
  form.address = String(d.address)
  form.overridesText = JSON.stringify(d.overrides ?? {}, null, 2)
  error.value = ''
}

watch(device, fillFromDevice, { immediate: true })

async function save(): Promise<void> {
  const d = device.value
  if (!d) return
  error.value = ''
  if (!form.channelId) {
    error.value = '请选择通道'
    return
  }
  if (!form.address.trim()) {
    error.value = '设备地址必填'
    return
  }
  let address: DeviceAddress = /^\d+$/.test(form.address.trim())
    ? Number(form.address.trim())
    : form.address.trim()
  if (pkg.value?.protocol.driver === 'modbus') {
    const n = Number(address)
    if (!Number.isInteger(n) || n < 1 || n > 247) {
      error.value = 'Modbus 从站地址需为 1–247 的整数'
      return
    }
  }
  let overrides: Record<string, unknown> = {}
  const text = form.overridesText.trim()
  if (text) {
    try {
      const parsed: unknown = JSON.parse(text)
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
        throw new Error('必须是 JSON 对象')
      }
      overrides = parsed as Record<string, unknown>
    } catch (e) {
      error.value = `overrides 不是合法 JSON：${errorMessage(e)}`
      return
    }
  }
  const config: DeviceConfig = {
    id: d.id,
    name: form.name.trim() || d.id,
    channelId: form.channelId,
    package: d.package,
    address,
    overrides,
  }
  saving.value = true
  try {
    await project.addDevice(config)
    toast.success(`设备 ${d.id} 参数已保存`)
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <section v-if="device" class="card dev-settings">
    <div class="card-head">
      <h3>参数设置</h3>
      <span class="muted small">修改后 gatewayd 会重建该通道的轮询计划</span>
    </div>

    <div class="dev-settings-body">
      <div class="form-grid">
        <div class="field">
          <label>设备名称</label>
          <n-input v-model:value="form.name" placeholder="例如：1#罐顶雷达" />
        </div>
        <div class="field">
          <label>通道</label>
          <n-select v-model:value="form.channelId" :options="channelOptions" />
        </div>
        <div class="field">
          <label>设备地址</label>
          <n-input v-model:value="form.address" placeholder="1 / AA:BB:CC:DD:EE:FF" />
        </div>
        <div class="field">
          <label>设备 ID（不可修改）</label>
          <n-input :value="device.id" disabled />
        </div>
        <div class="field span-2">
          <label>实例覆盖 overrides（JSON，可空）</label>
          <n-input
            v-model:value="form.overridesText"
            type="textarea"
            :autosize="{ minRows: 4, maxRows: 10 }"
            placeholder='{ "uab": { "scale": 0.01 } }'
          />
          <p class="hint">
            用于按点覆盖 scale / offset / pollGroup 等；具体字段由编译器和设备包点表解释。
          </p>
        </div>
      </div>

      <dl class="kv-list dev-settings-meta">
        <div class="kv">
          <dt>设备包</dt>
          <dd>{{ pkg?.name || device.package }} <span class="mono">v{{ pkg?.version }}</span></dd>
        </div>
        <div class="kv">
          <dt>点位</dt>
          <dd class="mono">{{ pointCount }}</dd>
        </div>
        <div class="kv">
          <dt>在线状态</dt>
          <dd>{{ device.online ? '在线' : '离线' }}</dd>
        </div>
        <div class="kv">
          <dt>驱动</dt>
          <dd class="mono">{{ pkg?.protocol.driver || '—' }}</dd>
        </div>
      </dl>

      <div v-if="error" class="banner danger">
        <AppIcon name="alert" :size="16" />
        <span>{{ error }}</span>
      </div>

      <div class="dev-settings-actions">
        <span class="muted small">切换通道/地址会短暂中断该链路上的轮询。</span>
        <span class="spacer" />
        <n-button size="small" @click="fillFromDevice">还原</n-button>
        <n-button
          size="small"
          type="primary"
          :disabled="!conn.connected"
          :loading="saving"
          @click="save"
        >
          保存参数
        </n-button>
      </div>
    </div>
  </section>

  <EmptyState
    v-else
    icon="chip"
    title="设备不存在"
    text="该设备可能已被删除，请返回设备页确认。"
  />
</template>
