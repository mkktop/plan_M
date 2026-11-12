<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NButton, NInput, NInputNumber, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import EmptyState from '@/components/EmptyState.vue'
import ModalShell from '@/components/ModalShell.vue'
import PageHeader from '@/components/PageHeader.vue'
import ChannelFormModal from '@/components/channel/ChannelFormModal.vue'
import ChannelScanModal from '@/components/channel/ChannelScanModal.vue'
import SerialPortsCard from '@/components/channel/SerialPortsCard.vue'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { errorMessage, useToastStore } from '@/stores/toast'
import { addressLabel, channelSummary, packageSupportsTransport } from '@/utils/transport'
import type { ChannelDto, DeviceAddress, DeviceDto, PackageDto } from '@/api/types'

// 设备页 = 总线（串口通道）+ 从站设备。通道是设备的父级，所以合并在一页：
// 每条总线一个分组，组头就是总线的状态/参数/扫描/编辑入口。
const route = useRoute()
const router = useRouter()
const project = useProjectStore()
const conn = useConnectionStore()
const toast = useToastStore()

const showForm = ref(false)
const saving = ref(false)
const idTouched = ref(false)
const pendingDelete = ref<DeviceDto | null>(null)
const deleting = ref(false)

// 总线（通道）
const channelFormOpen = ref(false)
const channelDraft = ref<ChannelDto | null>(null)
const channelInitialPort = ref<string | undefined>(undefined)
const scanChannel = ref<ChannelDto | null>(null)
const pendingChannelDelete = ref<ChannelDto | null>(null)
const deletingChannel = ref(false)

const form = reactive({
  channelId: '',
  packageId: '',
  /** 文本输入：Modbus 为 1–247，其他协议可为 MAC/路径 */
  address: '1',
  id: '',
  name: '',
})

const channelOptions = computed(() =>
  project.channels.map((c) => ({
    label: `${c.name || c.id}（${channelSummary(c)}）`,
    value: c.id,
  })),
)

const selectedChannel = computed(() => project.channels.find((c) => c.id === form.channelId))

/** 只列与当前通道传输兼容的包（驱动能力 ∩ 包声明）。 */
const compatiblePackages = computed<PackageDto[]>(() => {
  const ch = selectedChannel.value
  if (!ch) return project.packages
  return project.packages.filter((p) =>
    packageSupportsTransport(p, ch.transport, project.drivers),
  )
})

const packageOptions = computed(() =>
  compatiblePackages.value.map((p) => ({ label: `${p.name} · v${p.version}`, value: p.id })),
)

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

function onlineOn(channelId: string): number {
  return project.devicesOn(channelId).filter((d) => d.online).length
}

function pointCount(d: DeviceDto): number {
  const pkg = project.packageOf(d)
  return pkg ? pkg.points.reduce((n, p) => n + 1 + p.bits.length, 0) : 0
}

function parseAddress(raw: string): DeviceAddress {
  const s = raw.trim()
  return /^\d+$/.test(s) ? Number(s) : s
}

function addressSlug(raw: string): string {
  return raw.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'dev'
}

function autoFill(): void {
  if (idTouched.value) return
  const pkg = project.packageById.get(form.packageId)
  const slug = pkg?.id.split('.').filter(Boolean).pop() || 'dev'
  const base = `${slug}-${addressSlug(form.address)}`
  let id = base
  let n = 2
  while (project.devices.some((d) => d.id === id)) id = `${base}-${n++}`
  form.id = id
  form.name = pkg ? `${pkg.name} ${addressLabel(parseAddress(form.address))}` : `设备 ${form.address}`
}

watch([() => form.packageId, () => form.address], autoFill)
watch(selectedChannel, () => {
  if (form.packageId && !compatiblePackages.value.some((p) => p.id === form.packageId)) {
    form.packageId = compatiblePackages.value[0]?.id || ''
  }
})

function openAdd(
  channelId?: string,
  address?: DeviceAddress,
  packageId?: string,
): void {
  form.channelId = channelId || project.channels[0]?.id || ''
  const allow = compatiblePackages.value
  form.packageId =
    (packageId && allow.some((p) => p.id === packageId) ? packageId : '') ||
    allow[0]?.id ||
    ''
  form.address = address !== undefined ? String(address) : '1'
  idTouched.value = false
  autoFill()
  showForm.value = true
}

function validate(): string | null {
  if (!form.channelId) return '请选择通道'
  if (!form.packageId) return '请选择设备包（没有可用包时先导入）'
  if (!/^[A-Za-z0-9_-]+$/.test(form.id.trim())) return '设备 ID 只允许字母、数字、下划线、连字符'
  if (form.id.includes('.')) return '设备 ID 不能包含点号'
  if (project.devices.some((d) => d.id === form.id.trim())) return `设备 ID ${form.id.trim()} 已存在`
  if (!form.address.trim()) return '设备地址必填'
  const pkg = project.packageById.get(form.packageId)
  if (pkg?.protocol.driver === 'modbus') {
    const n = Number(form.address.trim())
    if (!Number.isInteger(n) || n < 1 || n > 247) return 'Modbus 从站地址需为 1–247 的整数'
  }
  return null
}

async function save(): Promise<void> {
  const err = validate()
  if (err) {
    toast.error(err)
    return
  }
  saving.value = true
  try {
    await project.addDevice({
      id: form.id.trim(),
      name: form.name.trim() || form.id.trim(),
      channelId: form.channelId,
      package: form.packageId,
      address: parseAddress(form.address),
    })
    toast.success(`设备 ${form.id.trim()} 已添加`)
    showForm.value = false
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    saving.value = false
  }
}

async function doDelete(): Promise<void> {
  const d = pendingDelete.value
  if (!d) return
  deleting.value = true
  try {
    await project.removeDevice(d.id)
    toast.success(`设备 ${d.id} 已删除`)
    pendingDelete.value = null
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    deleting.value = false
  }
}

function gotoTags(d: DeviceDto): void {
  router.push({ path: '/diagnostics', query: { tab: 'data', device: d.id } })
}

// —— 总线（通道）操作 ——

async function openChannelCreate(port?: string): Promise<void> {
  // 表单默认端口来自串口枚举；还没枚举过就先拉一次，失败也不阻断（可手填/SIM）
  if (conn.connected && !(project.targets.serial ?? []).length) {
    try {
      await project.loadSerialPorts()
    } catch {
      // 忽略：用户仍可选择 SIM 或手工输入串口名
    }
  }
  channelDraft.value = null
  channelInitialPort.value = port
  channelFormOpen.value = true
}

function openChannelEdit(ch: ChannelDto): void {
  channelDraft.value = ch
  channelInitialPort.value = undefined
  channelFormOpen.value = true
}

function requestChannelDelete(ch: ChannelDto): void {
  channelFormOpen.value = false
  pendingChannelDelete.value = ch
}

async function doDeleteChannel(): Promise<void> {
  const ch = pendingChannelDelete.value
  if (!ch) return
  deletingChannel.value = true
  try {
    await project.removeChannel(ch.id)
    toast.success(`通道 ${ch.id} 已删除`)
    pendingChannelDelete.value = null
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    deletingChannel.value = false
  }
}

function addDeviceFromScan(address: DeviceAddress): void {
  const ch = scanChannel.value
  scanChannel.value = null
  openAdd(ch?.id, address)
}

// 从设备包页 / 设备工作区跳转过来时自动打开添加对话框（快照可能已加载，也可能还在路上）。
const prefill = ref<{ channel?: string; address?: string; packageId?: string } | null>(null)

function applyPrefill(): void {
  if (!project.loaded || !prefill.value) return
  const p = prefill.value
  prefill.value = null
  openAdd(p.channel, p.address, p.packageId)
}

watch(() => project.loaded, applyPrefill, { immediate: true })
watch(prefill, applyPrefill)

onMounted(() => {
  const q = route.query
  if (q.add === '1') {
    prefill.value = {
      channel: typeof q.channel === 'string' ? q.channel : undefined,
      address: typeof q.address === 'string' ? q.address : undefined,
      packageId: typeof q.package === 'string' ? q.package : undefined,
    }
    void router.replace({ query: {} })
  }
})
</script>

<template>
  <div class="page">
    <PageHeader
      title="设备"
      subtitle="通道 = 串口 / TCP / 虚拟总线；设备 = 设备包 + 通道 + 协议地址"
    >
      <n-button size="small" @click="openChannelCreate()">
        <template #icon><AppIcon name="plug" :size="15" /></template>
        新建通道
      </n-button>
      <n-button size="small" @click="router.push('/packages')">
        <template #icon><AppIcon name="package" :size="15" /></template>
        设备包管理
      </n-button>
      <n-button
        size="small"
        type="primary"
        :disabled="!project.channels.length || !project.packages.length"
        @click="openAdd()"
      >
        <template #icon><AppIcon name="plus" :size="15" /></template>
        添加设备
      </n-button>
    </PageHeader>

    <div v-if="!conn.connected" class="banner warn">
      <AppIcon name="alert" :size="16" />
      <span>未连接 gatewayd，设备与总线的配置不可编辑。</span>
    </div>

    <div v-if="!project.packages.length && project.loaded" class="banner info">
      <AppIcon name="alert" :size="16" />
      <span>没有已安装的设备包。到「设备包」页导入 <code>manifest.json + points.json</code> 的目录或 zip。</span>
      <n-button size="tiny" quaternary @click="router.push('/packages')">去导入</n-button>
    </div>

    <template v-if="project.channels.length">
      <section v-for="ch in project.channels" :key="ch.id" class="channel-group">
        <header class="group-head">
          <span class="state-dot" :class="stateOf(ch).tone" :title="stateOf(ch).label" />
          <h3>{{ ch.name || ch.id }}</h3>
          <code>{{ ch.id }} · {{ channelSummary(ch) }}</code>
          <span class="state-pill" :class="stateOf(ch).tone">{{ stateOf(ch).label }}</span>
          <span v-if="ch.configError" class="state-pill bad" :title="ch.configError">配置错误</span>
          <div class="spacer" />
          <span class="muted small">
            {{ project.devicesOn(ch.id).length }} 台 · 在线 {{ onlineOn(ch.id) }}
          </span>
          <n-button size="tiny" quaternary @click="scanChannel = ch">
            <template #icon><AppIcon name="scan" :size="14" /></template>
            扫描
          </n-button>
          <n-button size="tiny" quaternary @click="openChannelEdit(ch)">编辑</n-button>
          <n-button size="tiny" quaternary @click="openAdd(ch.id)">
            <template #icon><AppIcon name="plus" :size="14" /></template>
            添加设备
          </n-button>
        </header>

        <div class="device-grid">
          <article v-for="d in project.devicesOn(ch.id)" :key="d.id" class="card device-card">
            <div class="device-top">
              <span class="online-dot" :class="{ on: d.online }" :title="d.online ? '在线' : '离线'" />
              <div class="device-title">
                <strong>{{ d.name || d.id }}</strong>
                <code>{{ d.id }}</code>
              </div>
              <span class="slave-badge mono">{{ addressLabel(d.address) }}</span>
            </div>

            <div class="device-meta">
              <div class="device-pkg">
                <span class="label">设备包</span>
                <span>{{ project.packageOf(d)?.name ?? d.package }}</span>
                <span class="mono muted">v{{ project.packageOf(d)?.version ?? '—' }}</span>
              </div>
              <div>
                <span class="label">点位</span>
                <span class="mono">{{ pointCount(d) }}</span>
              </div>
              <div>
                <span class="label">状态</span>
                <span :class="d.online ? 'text-good' : 'text-muted'">{{ d.online ? '在线' : '离线' }}</span>
              </div>
            </div>

            <div v-if="d.configError" class="banner warn mt-8 small">
              <AppIcon name="alert" :size="14" />
              <span>{{ d.configError }}</span>
            </div>

            <div class="device-foot">
              <n-button size="tiny" @click="gotoTags(d)">
                <template #icon><AppIcon name="activity" :size="14" /></template>
                实时点表
              </n-button>
              <div class="spacer" />
              <n-button size="tiny" quaternary type="error" @click="pendingDelete = d">删除</n-button>
            </div>
          </article>

          <button type="button" class="device-add-tile" @click="openAdd(ch.id)">
            <AppIcon name="plus" :size="20" />
            <span>添加设备</span>
          </button>
        </div>

        <div v-if="!project.devicesOn(ch.id).length" class="group-empty muted">
          这条总线还没有从站。接好设备后点「添加设备」，或用「扫描」自动找地址。
        </div>
      </section>
    </template>

    <EmptyState
      v-else-if="project.loaded"
      icon="plug"
      title="还没有通道"
      text="接入 USB-485 / TCP 网关后建一条通道；没有硬件时可用虚拟总线演示。"
    >
      <n-button size="small" type="primary" @click="openChannelCreate()">新建通道</n-button>
    </EmptyState>

    <SerialPortsCard @use="openChannelCreate" />

    <!-- 新建/编辑通道 -->
    <ChannelFormModal
      v-if="channelFormOpen"
      :channel="channelDraft"
      :initial-port="channelInitialPort"
      @close="channelFormOpen = false"
      @remove="requestChannelDelete"
    />

    <!-- 扫描从站 -->
    <ChannelScanModal
      v-if="scanChannel"
      :channel="scanChannel"
      @close="scanChannel = null"
      @pick="addDeviceFromScan"
    />

    <!-- 添加设备 -->
    <ModalShell v-if="showForm" title="添加设备" subtitle="设备 ID 是 tag 前缀，添加后不可与已有设备重复" :width="560" @close="showForm = false">
      <div v-if="!project.channels.length || !project.packages.length" class="banner warn mb-12">
        <AppIcon name="alert" :size="16" />
        <span>
          {{ !project.channels.length ? '还没有可用通道。' : '' }}
          {{ !project.packages.length ? '还没有已安装的设备包。' : '' }}
        </span>
      </div>
      <div class="form-grid">
        <div class="field span-2">
          <label>通道</label>
          <n-select v-model:value="form.channelId" :options="channelOptions" placeholder="选择串口通道" />
        </div>
        <div class="field span-2">
          <label>设备包</label>
          <n-select v-model:value="form.packageId" :options="packageOptions" placeholder="选择设备型号" />
        </div>
        <div class="field">
          <label>设备地址</label>
          <n-input v-model:value="form.address" placeholder="1 / AA:BB:CC:DD:EE:FF" />
          <p class="hint">Modbus 为 1–247；其他协议可填文本地址。</p>
        </div>
        <div class="field">
          <label>设备 ID</label>
          <n-input v-model:value="form.id" placeholder="pm3000-1" @update:value="idTouched = true" />
        </div>
        <div class="field span-2">
          <label>显示名</label>
          <n-input v-model:value="form.name" placeholder="1# 进线电表" />
        </div>
      </div>
      <template #footer>
        <n-button size="small" @click="showForm = false">取消</n-button>
        <n-button size="small" type="primary" :loading="saving" :disabled="!project.channels.length || !project.packages.length" @click="save">
          添加
        </n-button>
      </template>
    </ModalShell>

    <!-- 删除设备 -->
    <ModalShell v-if="pendingDelete" title="删除设备" :width="440" @close="pendingDelete = null">
      <p>确认删除设备 <b class="mono">{{ pendingDelete.id }}</b>（{{ pendingDelete.name }}）？</p>
      <p class="muted mt-8">该设备的实时 tag 会从采集计划中移除。</p>
      <template #footer>
        <n-button size="small" @click="pendingDelete = null">取消</n-button>
        <n-button size="small" type="error" :loading="deleting" @click="doDelete">删除</n-button>
      </template>
    </ModalShell>

    <!-- 删除通道（连带从站） -->
    <ModalShell
      v-if="pendingChannelDelete"
      title="删除通道"
      :width="440"
      @close="pendingChannelDelete = null"
    >
      <p>
        确认删除通道 <b class="mono">{{ pendingChannelDelete.id }}</b>（{{ channelSummary(pendingChannelDelete) }}）？
      </p>
      <p class="text-warn mt-8">
        挂在该通道下的 {{ project.devicesOn(pendingChannelDelete.id).length }} 台设备会一并删除，且不可撤销。
      </p>
      <template #footer>
        <n-button size="small" @click="pendingChannelDelete = null">取消</n-button>
        <n-button size="small" type="error" :loading="deletingChannel" @click="doDeleteChannel">删除</n-button>
      </template>
    </ModalShell>
  </div>
</template>
