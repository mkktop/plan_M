<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { NButton, NInput, NInputNumber, NSelect } from 'naive-ui'
import ModalShell from '@/components/ModalShell.vue'
import { useProjectStore } from '@/stores/project'
import { errorMessage, useToastStore } from '@/stores/toast'
import type { ChannelDto, Parity, SerialChannelConfig } from '@/api/types'

// 总线（串口通道）的新建/编辑。一条串口 = 一条 485 总线，挂 N 台从站。
const props = defineProps<{ channel: ChannelDto | null; initialPort?: string }>()
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
  port: string
  hwId: string
  baudRate: number
  dataBits: number
  parity: Parity
  stopBits: number
  respTimeoutMs: number
  retries: number
  interFrameGapMs: number
}

const blank = (): ChannelForm => ({
  id: '',
  name: '',
  port: '',
  hwId: '',
  baudRate: 9600,
  dataBits: 8,
  parity: 'none',
  stopBits: 1,
  respTimeoutMs: 200,
  retries: 2,
  interFrameGapMs: 5,
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
  return preferred || project.serialPorts.find((x) => x.portName !== 'SIM')?.portName || 'SIM'
}

function fromChannel(ch: ChannelDto): ChannelForm {
  return {
    id: ch.id,
    name: ch.name,
    port: ch.port,
    hwId: ch.hwId ?? '',
    baudRate: ch.baudRate,
    dataBits: ch.dataBits,
    parity: ch.parity,
    stopBits: ch.stopBits,
    respTimeoutMs: ch.respTimeoutMs,
    retries: ch.retries,
    interFrameGapMs: ch.interFrameGapMs,
  }
}

function newChannelForm(preferredPort?: string): ChannelForm {
  const p = defaultPort(preferredPort)
  return {
    ...blank(),
    port: p,
    id: slugPort(p),
    name: p === 'SIM' ? '演示总线' : `总线 ${p}`,
  }
}

const editing = props.channel
const form = reactive<ChannelForm>(
  editing ? fromChannel(editing) : newChannelForm(props.initialPort),
)
const saving = ref(false)

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
  const opts = project.serialPorts.map((p) => ({
    label: `${p.portName} · ${p.description || '串口'}`,
    value: p.portName,
  }))
  if (!opts.some((o) => o.value === 'SIM')) {
    opts.push({ label: 'SIM · 虚拟总线（演示）', value: 'SIM' })
  }
  if (form.port && !opts.some((o) => o.value === form.port)) {
    opts.unshift({ label: form.port, value: form.port })
  }
  return opts
})

function validate(): string | null {
  if (!form.id.trim()) return '通道 ID 必填'
  if (form.id.includes('.')) return '通道 ID 不能包含点号（tag 用点号分段）'
  if (!/^[A-Za-z0-9_-]+$/.test(form.id.trim()))
    return '通道 ID 只允许字母、数字、下划线、连字符'
  if (!form.port.trim()) return '请选择或填写串口'
  if (!form.baudRate || form.baudRate <= 0) return '波特率必须大于 0'
  if (form.respTimeoutMs < 20) return '应答超时至少 20ms'
  return null
}

async function save(): Promise<void> {
  const err = validate()
  if (err) {
    toast.error(err)
    return
  }
  const cfg: SerialChannelConfig = {
    id: form.id.trim(),
    name: form.name.trim() || form.id.trim(),
    port: form.port.trim(),
    hwId: form.hwId.trim() || null,
    baudRate: form.baudRate,
    dataBits: form.dataBits,
    parity: form.parity,
    stopBits: form.stopBits,
    respTimeoutMs: form.respTimeoutMs,
    retries: form.retries,
    interFrameGapMs: form.interFrameGapMs,
  }
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
    subtitle="改动会保存到工程配置并立即重启该通道"
    :width="620"
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
        <n-input v-model:value="form.name" placeholder="总线 COM3" />
      </div>
      <div class="field">
        <label>串口</label>
        <n-select v-model:value="form.port" :options="portOptions" tag filterable placeholder="COM3 / SIM" />
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
        <label>应答超时（ms）</label>
        <n-input-number v-model:value="form.respTimeoutMs" :min="20" :max="10000" style="width: 100%" />
      </div>
      <div class="field">
        <label>重试次数</label>
        <n-input-number v-model:value="form.retries" :min="0" :max="10" style="width: 100%" />
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
    </div>
    <template #footer>
      <n-button v-if="editing" size="small" quaternary type="error" @click="emit('remove', editing)">
        删除这条总线
      </n-button>
      <div class="spacer" />
      <n-button size="small" @click="emit('close')">取消</n-button>
      <n-button size="small" type="primary" :loading="saving" @click="save">
        {{ editing ? '保存并重启' : '创建' }}
      </n-button>
    </template>
  </ModalShell>
</template>
