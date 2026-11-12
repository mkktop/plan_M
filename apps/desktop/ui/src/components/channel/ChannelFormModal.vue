<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { NButton, NInput, NInputNumber, NSelect } from 'naive-ui'
import ModalShell from '@/components/ModalShell.vue'
import { useProjectStore } from '@/stores/project'
import { errorMessage, useToastStore } from '@/stores/toast'
import { TRANSPORT_LABELS, transportLabel } from '@/utils/transport'
import type { ChannelConfig, ChannelDto, Parity, TransportKind } from '@/api/types'

// 通道 = 一条传输连接（串口 / TCP / 虚拟 / BLE），设备挂在通道上。
const props = defineProps<{
  channel: ChannelDto | null
  initialPort?: string
  initialTransport?: TransportKind
}>()
const emit = defineEmits<{
  close: []
  remove: [channel: ChannelDto]
  saved: [id: string]
}>()

const project = useProjectStore()
const toast = useToastStore()

interface ChannelForm {
  id: string
  name: string
  transport: TransportKind
  // common timing
  respTimeoutMs: number
  retries: number
  probeIntervalMs: number
  // serial
  port: string
  hwId: string
  baudRate: number
  dataBits: number
  parity: Parity
  stopBits: number
  interFrameGapMs: number
  // tcp
  host: string
  tcpPort: number
  connectTimeoutMs: number
  // ble
  adapter: string
  bleAddress: string
}

const blank = (): ChannelForm => ({
  id: '',
  name: '',
  transport: 'serial',
  respTimeoutMs: 200,
  retries: 2,
  probeIntervalMs: 5000,
  port: '',
  hwId: '',
  baudRate: 9600,
  dataBits: 8,
  parity: 'none',
  stopBits: 1,
  interFrameGapMs: 5,
  host: '',
  tcpPort: 502,
  connectTimeoutMs: 3000,
  adapter: '',
  bleAddress: '',
})

function slugPort(p: string): string {
  return (
    p
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'bus'
  )
}

function defaultPort(preferred?: string): string {
  return preferred || project.targets.serial?.[0]?.id || 'COM3'
}

function supportedTransports(): TransportKind[] {
  if (project.transports.length) return project.transports
  return ['serial']
}

function defaultTransport(): TransportKind {
  return props.initialTransport && supportedTransports().includes(props.initialTransport)
    ? props.initialTransport
    : (supportedTransports()[0] ?? 'serial')
}

function fromChannel(ch: ChannelDto): ChannelForm {
  const f = { ...blank(), id: ch.id, name: ch.name, transport: ch.transport }
  f.respTimeoutMs = ch.timing.respTimeoutMs
  f.retries = ch.timing.retries
  f.probeIntervalMs = ch.timing.probeIntervalMs
  switch (ch.transport) {
    case 'serial':
      f.port = ch.port
      f.hwId = ch.hwId ?? ''
      f.baudRate = ch.baudRate
      f.dataBits = ch.dataBits
      f.parity = ch.parity
      f.stopBits = ch.stopBits
      f.interFrameGapMs = ch.interFrameGapMs
      break
    case 'tcp':
      f.host = ch.host
      f.tcpPort = ch.port
      f.connectTimeoutMs = ch.connectTimeoutMs
      break
    case 'ble':
      f.adapter = ch.adapter ?? ''
      f.bleAddress = ch.address
      break
    default:
      break
  }
  return f
}

function newChannelForm(preferredPort?: string): ChannelForm {
  const transport = defaultTransport()
  const f = { ...blank(), transport }
  if (transport === 'serial') {
    f.port = defaultPort(preferredPort)
    f.id = slugPort(f.port)
    f.name = `总线 ${f.port}`
  } else if (transport === 'tcp') {
    f.id = 'tcp-1'
    f.name = 'TCP 网关'
  } else if (transport === 'sim') {
    f.id = 'sim1'
    f.name = '虚拟总线'
  } else {
    f.id = 'ble-1'
    f.name = '蓝牙设备'
  }
  return f
}

const editing = props.channel
const form = reactive<ChannelForm>(
  editing ? fromChannel(editing) : newChannelForm(props.initialPort),
)
const saving = ref(false)

const transportOptions = computed(() => {
  const kinds = supportedTransports().slice()
  // 编辑旧配置时，即使宿主当前未实现该传输也要能看见/保存
  if (editing && !kinds.includes(editing.transport)) kinds.push(editing.transport)
  return kinds.map((k) => ({ label: TRANSPORT_LABELS[k] ?? k, value: k }))
})

const baudOptions = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200].map((v) => ({
  label: String(v),
  value: v,
}))
const parityOptions = [
  { label: '无校验 N', value: 'none' },
  { label: '奇校验 O', value: 'odd' },
  { label: '偶校验 E', value: 'even' },
]
const dataBitsOptions = [8, 7].map((v) => ({ label: `${v} 数据位`, value: v }))
const stopBitsOptions = [1, 2].map((v) => ({ label: `${v} 停止位`, value: v }))

const portOptions = computed(() => {
  const opts = (project.targets.serial ?? []).map((p) => ({
    label: `${p.label}${p.description ? ` · ${p.description}` : ''}`,
    value: p.id,
  }))
  if (form.port && !opts.some((o) => o.value === form.port)) {
    opts.unshift({ label: form.port, value: form.port })
  }
  return opts
})

onMounted(() => {
  if (form.transport === 'serial') void project.loadTransportTargets('serial')
})

watch(
  () => form.transport,
  (kind) => {
    if (kind === 'serial') void project.loadTransportTargets('serial')
  },
)

function validate(): string | null {
  if (!form.id.trim()) return '通道 ID 必填'
  if (form.id.includes('.')) return '通道 ID 不能包含点号（tag 用点号分段）'
  if (!/^[A-Za-z0-9_-]+$/.test(form.id.trim()))
    return '通道 ID 只允许字母、数字、下划线、连字符'
  if (form.respTimeoutMs < 20) return '应答超时至少 20ms'
  if (form.probeIntervalMs < 100) return '离线探测周期至少 100ms'
  if (form.transport === 'serial') {
    if (!form.port.trim()) return '请选择或填写串口'
    if (!form.baudRate || form.baudRate <= 0) return '波特率必须大于 0'
  } else if (form.transport === 'tcp') {
    if (!form.host.trim()) return 'TCP 主机地址必填'
    if (!form.tcpPort || form.tcpPort < 1 || form.tcpPort > 65535) return 'TCP 端口应为 1-65535'
    if (form.connectTimeoutMs < 100) return '连接超时至少 100ms'
  } else if (form.transport === 'ble') {
    if (!form.bleAddress.trim()) return 'BLE 外设地址必填'
  }
  return null
}

function buildConfig(): ChannelConfig {
  const base = {
    id: form.id.trim(),
    name: form.name.trim() || form.id.trim(),
    timing: {
      respTimeoutMs: form.respTimeoutMs,
      retries: form.retries,
      probeIntervalMs: form.probeIntervalMs,
    },
  }
  switch (form.transport) {
    case 'serial':
      return {
        ...base,
        transport: 'serial',
        port: form.port.trim(),
        hwId: form.hwId.trim() || null,
        baudRate: form.baudRate,
        dataBits: form.dataBits,
        parity: form.parity,
        stopBits: form.stopBits,
        interFrameGapMs: form.interFrameGapMs,
      }
    case 'tcp':
      return {
        ...base,
        transport: 'tcp',
        host: form.host.trim(),
        port: form.tcpPort,
        connectTimeoutMs: form.connectTimeoutMs,
      }
    case 'ble':
      return {
        ...base,
        transport: 'ble',
        adapter: form.adapter.trim() || null,
        address: form.bleAddress.trim(),
      }
    default:
      return { ...base, transport: 'sim', seed: null }
  }
}

async function save(): Promise<void> {
  const err = validate()
  if (err) {
    toast.error(err)
    return
  }
  const cfg = buildConfig()
  saving.value = true
  try {
    await project.upsertChannel(cfg)
    toast.success(editing ? '通道已更新（已重启）' : '通道已创建')
    emit('saved', cfg.id)
    emit('close')
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <ModalShell
    :title="editing ? '编辑通道' : '新建通道'"
    subtitle="通道是一条传输连接；改动会保存到工程配置并立即重启"
    :width="640"
    @close="emit('close')"
  >
    <div class="form-grid">
      <div class="field">
        <label>通道 ID</label>
        <n-input v-model:value="form.id" :disabled="!!editing" placeholder="ch-com3" />
        <p class="hint">tag 不依赖它；只允许字母数字下划线连字符。</p>
      </div>
      <div class="field">
        <label>显示名</label>
        <n-input v-model:value="form.name" placeholder="现场总线" />
      </div>
      <div class="field">
        <label>传输类型</label>
        <n-select v-model:value="form.transport" :options="transportOptions" />
        <p class="hint">列表由宿主 Welcome 能力上报；未实现的传输不会出现。</p>
      </div>
      <div class="field">
        <label>应答超时（ms）</label>
        <n-input-number v-model:value="form.respTimeoutMs" :min="20" :max="10000" style="width: 100%" />
      </div>
      <div class="field">
        <label>重试次数</label>
        <n-input-number v-model:value="form.retries" :min="0" :max="10" style="width: 100%" />
      </div>
      <div class="field">
        <label>离线探测周期（ms）</label>
        <n-input-number v-model:value="form.probeIntervalMs" :min="100" :max="600000" style="width: 100%" />
      </div>

      <template v-if="form.transport === 'serial'">
        <div class="field">
          <label>串口</label>
          <n-select v-model:value="form.port" :options="portOptions" tag filterable placeholder="COM3" />
        </div>
        <div class="field">
          <label>波特率</label>
          <n-select v-model:value="form.baudRate" :options="baudOptions" tag filterable />
        </div>
        <div class="field">
          <label>数据位</label>
          <n-select v-model:value="form.dataBits" :options="dataBitsOptions" />
        </div>
        <div class="field">
          <label>校验位</label>
          <n-select v-model:value="form.parity" :options="parityOptions" />
        </div>
        <div class="field">
          <label>停止位</label>
          <n-select v-model:value="form.stopBits" :options="stopBitsOptions" />
        </div>
        <div class="field">
          <label>帧间静默（ms）</label>
          <n-input-number v-model:value="form.interFrameGapMs" :min="0" :max="1000" style="width: 100%" />
        </div>
        <div class="field span-2">
          <label>硬件 ID（可选）</label>
          <n-input v-model:value="form.hwId" placeholder="VID_1A86&PID_7523" />
          <p class="hint">填入后 USB 重新插拔导致 COM 号变化时，按该 ID 自动找回设备。</p>
        </div>
      </template>

      <template v-else-if="form.transport === 'tcp'">
        <div class="field">
          <label>主机地址</label>
          <n-input v-model:value="form.host" placeholder="192.168.1.10" />
        </div>
        <div class="field">
          <label>端口</label>
          <n-input-number v-model:value="form.tcpPort" :min="1" :max="65535" style="width: 100%" />
        </div>
        <div class="field">
          <label>连接超时（ms）</label>
          <n-input-number v-model:value="form.connectTimeoutMs" :min="100" :max="60000" style="width: 100%" />
        </div>
      </template>

      <template v-else-if="form.transport === 'ble'">
        <div class="field">
          <label>适配器（可选）</label>
          <n-input v-model:value="form.adapter" placeholder="留空 = 系统默认" />
        </div>
        <div class="field">
          <label>外设地址</label>
          <n-input v-model:value="form.bleAddress" placeholder="AA:BB:CC:DD:EE:FF" />
        </div>
        <p class="hint span-2">BLE 传输由宿主能力决定；当前版本预留接口，未实现时创建会被拒绝。</p>
      </template>

      <template v-else>
        <p class="hint span-2">
          {{ transportLabel('sim') }}：进程内虚拟总线，使用宿主内置的演示从站（无需硬件）。
        </p>
      </template>
    </div>
    <template #footer>
      <n-button v-if="editing" size="small" quaternary type="error" @click="emit('remove', editing)">
        删除这条通道
      </n-button>
      <div class="spacer" />
      <n-button size="small" @click="emit('close')">取消</n-button>
      <n-button size="small" type="primary" :loading="saving" @click="save">
        {{ editing ? '保存并重启' : '创建' }}
      </n-button>
    </template>
  </ModalShell>
</template>
