<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'
import { NButton, NInput, NInputNumber, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import EmptyState from '@/components/EmptyState.vue'
import ModalShell from '@/components/ModalShell.vue'
import PageHeader from '@/components/PageHeader.vue'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { errorMessage, useToastStore } from '@/stores/toast'
import type { ChannelDto, Parity, SerialChannelConfig } from '@/api/types'

const router = useRouter()
const project = useProjectStore()
const conn = useConnectionStore()
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

const form = reactive<ChannelForm>(blank())
const showForm = ref(false)
const editingId = ref<string | null>(null)
const saving = ref(false)
const portsLoading = ref(false)
const pendingDelete = ref<ChannelDto | null>(null)
const deleting = ref(false)

const scanOpen = ref(false)
const scanChannelId = ref('')
const scanFrom = ref(1)
const scanTo = ref(32)
const scanning = ref(false)
const scanFound = ref<number[]>([])
const scanError = ref('')

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

const STATE_META: Record<string, { label: string; tone: string }> = {
  running: { label: '运行中', tone: 'ok' },
  recovering: { label: '重连中', tone: 'warn' },
  faulted: { label: '故障', tone: 'bad' },
  stopped: { label: '已停止', tone: 'idle' },
}

function stateOf(ch: ChannelDto): { label: string; tone: string } {
  const s = project.channelStates[ch.id] ?? ch.state
  return STATE_META[s] ?? { label: s, tone: 'idle' }
}

function parityLetter(p: Parity): string {
  return p === 'none' ? 'N' : p === 'odd' ? 'O' : 'E'
}

function onlineOn(channelId: string): number {
  return project.devicesOn(channelId).filter((d) => d.online).length
}

function slugPort(p: string): string {
  return (
    p
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'bus'
  )
}

function openCreate(port?: string): void {
  Object.assign(form, blank())
  const p = port || project.serialPorts.find((x) => x.portName !== 'SIM')?.portName || 'SIM'
  form.port = p
  form.id = slugPort(p)
  form.name = p === 'SIM' ? '演示总线' : `总线 ${p}`
  editingId.value = null
  showForm.value = true
}

function openEdit(ch: ChannelDto): void {
  Object.assign(form, {
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
  })
  editingId.value = ch.id
  showForm.value = true
}

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
    toast.success(editingId.value ? '通道已更新（已重启）' : '通道已创建')
    showForm.value = false
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    saving.value = false
  }
}

async function loadPorts(): Promise<void> {
  if (!conn.connected) {
    toast.error('先连接 gatewayd 才能枚举串口')
    return
  }
  portsLoading.value = true
  try {
    await project.loadSerialPorts()
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    portsLoading.value = false
  }
}

function openScan(ch: ChannelDto): void {
  scanChannelId.value = ch.id
  scanFrom.value = 1
  scanTo.value = 32
  scanFound.value = []
  scanError.value = ''
  scanOpen.value = true
}

async function runScan(): Promise<void> {
  if (scanFrom.value < 1 || scanTo.value > 247 || scanFrom.value > scanTo.value) {
    scanError.value = '地址范围需在 1–247 且起始不大于结束'
    return
  }
  scanning.value = true
  scanFound.value = []
  scanError.value = ''
  try {
    scanFound.value = await project.scanChannel(scanChannelId.value, [scanFrom.value, scanTo.value])
    if (scanFound.value.length === 0) {
      scanError.value = '未发现从站：检查接线、波特率/校验、从站地址范围'
    }
  } catch (e) {
    scanError.value = errorMessage(e)
  } finally {
    scanning.value = false
  }
}

function addDeviceFromScan(slave: number): void {
  scanOpen.value = false
  router.push({ path: '/devices', query: { add: '1', channel: scanChannelId.value, slave: String(slave) } })
}

async function doDelete(): Promise<void> {
  const ch = pendingDelete.value
  if (!ch) return
  deleting.value = true
  try {
    await project.removeChannel(ch.id)
    toast.success(`通道 ${ch.id} 已删除`)
    pendingDelete.value = null
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    deleting.value = false
  }
}

onMounted(() => {
  if (conn.connected) void loadPorts()
})
</script>

<template>
  <div class="page">
    <PageHeader title="通道" subtitle="一条串口对应一条 485 总线，总线上可挂多台从站设备">
      <n-button size="small" :loading="portsLoading" @click="loadPorts">
        <template #icon><AppIcon name="refresh" :size="15" /></template>
        刷新串口
      </n-button>
      <n-button size="small" type="primary" @click="openCreate()">
        <template #icon><AppIcon name="plus" :size="15" /></template>
        新建通道
      </n-button>
    </PageHeader>

    <div v-if="!conn.connected" class="banner warn">
      <AppIcon name="alert" :size="16" />
      <span>未连接 gatewayd，配置不可编辑。请到「诊断」页连接后再操作。</span>
    </div>

    <div v-if="project.channels.length" class="channel-grid">
      <article v-for="ch in project.channels" :key="ch.id" class="card channel-card">
        <div class="channel-top">
          <div class="channel-title">
            <span class="state-dot" :class="stateOf(ch).tone" />
            <h3>{{ ch.name || ch.id }}</h3>
            <code>{{ ch.id }}</code>
          </div>
          <span class="state-pill" :class="stateOf(ch).tone">{{ stateOf(ch).label }}</span>
        </div>

        <div class="channel-port-row">
          <span class="channel-port mono">{{ ch.port }}</span>
          <span class="muted small">{{ ch.baudRate }} bps · {{ ch.dataBits }}{{ parityLetter(ch.parity) }}{{ ch.stopBits }}</span>
        </div>

        <div class="channel-params">
          <span>应答超时 <b class="mono">{{ ch.respTimeoutMs }}</b> ms</span>
          <span>重试 <b class="mono">{{ ch.retries }}</b> 次</span>
          <span>帧间隔 <b class="mono">{{ ch.interFrameGapMs }}</b> ms</span>
        </div>

        <div class="channel-foot">
          <span class="muted">
            {{ project.devicesOn(ch.id).length }} 台设备 · 在线 {{ onlineOn(ch.id) }}
          </span>
          <div class="row gap-4">
            <n-button size="tiny" quaternary @click="openScan(ch)">
              <template #icon><AppIcon name="scan" :size="14" /></template>
              扫描
            </n-button>
            <n-button size="tiny" quaternary @click="openEdit(ch)">编辑</n-button>
            <n-button size="tiny" quaternary type="error" @click="pendingDelete = ch">删除</n-button>
          </div>
        </div>
      </article>
    </div>

    <EmptyState
      v-else-if="project.loaded"
      icon="plug"
      title="还没有串口通道"
      text="接入 USB-485 后新建通道；没有硬件时可用 SIM 虚拟总线演示。"
    >
      <n-button size="small" type="primary" @click="openCreate()">新建通道</n-button>
    </EmptyState>

    <section class="card ports-card">
      <div class="card-head">
        <h3>本机串口</h3>
        <span class="muted">按硬件 ID 匹配可防止 COM 号漂移</span>
      </div>
      <div v-if="project.serialPorts.length" class="port-list">
        <div v-for="p in project.serialPorts" :key="p.portName" class="port-row">
          <div class="port-name mono">{{ p.portName }}</div>
          <div class="port-desc">
            <span>{{ p.description || '串行设备' }}</span>
            <code v-if="p.hwId" class="hw-id">{{ p.hwId }}</code>
          </div>
          <n-button size="tiny" @click="openCreate(p.portName)">用此口建通道</n-button>
        </div>
      </div>
      <div v-else class="port-empty muted">
        {{ portsLoading ? '正在枚举串口…' : '未发现串口（检查 USB-485 是否插好，或点击右上角刷新）' }}
      </div>
    </section>

    <!-- 新建/编辑通道 -->
    <ModalShell
      v-if="showForm"
      :title="editingId ? '编辑通道' : '新建通道'"
      subtitle="改动会保存到工程配置并立即重启该通道"
      :width="620"
      @close="showForm = false"
    >
      <div class="form-grid">
        <div class="field">
          <label>通道 ID</label>
          <n-input v-model:value="form.id" :disabled="!!editingId" placeholder="ch-com3" />
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
        <n-button size="small" @click="showForm = false">取消</n-button>
        <n-button size="small" type="primary" :loading="saving" @click="save">保存</n-button>
      </template>
    </ModalShell>

    <!-- 扫描从站 -->
    <ModalShell
      v-if="scanOpen"
      title="扫描从站"
      :subtitle="`通道 ${scanChannelId} · 探测与轮询串行执行，请等待`"
      :width="560"
      @close="scanOpen = false"
    >
      <div class="scan-controls">
        <div class="field">
          <label>起始地址</label>
          <n-input-number v-model:value="scanFrom" :min="1" :max="247" style="width: 120px" />
        </div>
        <div class="field">
          <label>结束地址</label>
          <n-input-number v-model:value="scanTo" :min="1" :max="247" style="width: 120px" />
        </div>
        <n-button type="primary" size="small" :loading="scanning" :disabled="scanning" @click="runScan">
          <template #icon><AppIcon name="scan" :size="15" /></template>
          {{ scanning ? '扫描中…' : '开始扫描' }}
        </n-button>
      </div>
      <div v-if="scanError" class="banner warn mt-12">
        <AppIcon name="alert" :size="16" />
        <span>{{ scanError }}</span>
      </div>
      <div v-if="scanFound.length" class="scan-result">
        <div class="muted small">发现 {{ scanFound.length }} 个从站</div>
        <div class="scan-chips">
          <div v-for="s in scanFound" :key="s" class="scan-chip">
            <span class="mono">#{{ s }}</span>
            <n-button size="tiny" type="primary" quaternary @click="addDeviceFromScan(s)">添加设备</n-button>
          </div>
        </div>
      </div>
      <p class="hint mt-12">
        扫描会逐个从站探测，1–32 约需十几秒。若全部无响应，优先核对波特率、校验位与 A/B 线序。
      </p>
      <template #footer>
        <n-button size="small" @click="scanOpen = false">关闭</n-button>
      </template>
    </ModalShell>

    <!-- 删除通道 -->
    <ModalShell
      v-if="pendingDelete"
      title="删除通道"
      :width="440"
      @close="pendingDelete = null"
    >
      <p>
        确认删除通道 <b class="mono">{{ pendingDelete.id }}</b>（{{ pendingDelete.port }}）？
      </p>
      <p class="text-warn mt-8">
        挂在该通道下的 {{ project.devicesOn(pendingDelete.id).length }} 台设备会一并删除，且不可撤销。
      </p>
      <template #footer>
        <n-button size="small" @click="pendingDelete = null">取消</n-button>
        <n-button size="small" type="error" :loading="deleting" @click="doDelete">删除</n-button>
      </template>
    </ModalShell>
  </div>
</template>
