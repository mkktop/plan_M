<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NButton, NInput, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import EmptyState from '@/components/EmptyState.vue'
import ModalShell from '@/components/ModalShell.vue'
import PageHeader from '@/components/PageHeader.vue'
import ChannelFormModal from '@/components/channel/ChannelFormModal.vue'
import ChannelScanModal from '@/components/channel/ChannelScanModal.vue'
import SerialPortsCard from '@/components/channel/SerialPortsCard.vue'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { useTagsStore } from '@/stores/tags'
import { errorMessage, useToastStore } from '@/stores/toast'
import { nowMs } from '@/utils/clock'
import { pointCount } from '@/utils/devicePoints'
import { formatClock } from '@/utils/format'
import { addressLabel, channelStatus, channelSummary, packageSupportsTransport, transportLabel } from '@/utils/transport'
import type { ChannelDto, DeviceAddress, DeviceDto, PackageDto } from '@/api/types'

// 设备页 = 总线（串口通道）+ 从站设备。通道是设备的父级，所以合并在一页：
// 每条总线一个分组，组头就是总线的状态/参数/扫描/编辑入口。
const route = useRoute()
const router = useRouter()
const project = useProjectStore()
const conn = useConnectionStore()
const toast = useToastStore()
const tags = useTagsStore()

const showForm = ref(false)
const saving = ref(false)
const saveError = ref('')
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
const firstDevice = computed(() => project.loaded && !project.devices.length)
const compatibleChannel = computed(() => project.channels.find((channel) =>
  project.packages.some((pkg) => packageSupportsTransport(pkg, channel.transport, project.drivers)),
))
const verificationId = ref('')
const verificationStartedAt = ref(0)
const verifiedAt = ref<number>()
const verificationWaiting = computed(() => nowMs.value - verificationStartedAt.value < 10_000)
const verificationDevice = computed(() => project.devices.find((device) => device.id === verificationId.value))
const verificationTags = computed(() => {
  const device = verificationDevice.value
  if (!device || verifiedAt.value !== undefined) return []
  return project.packageOf(device)?.points.map((point) => `${device.id}.${point.id}`) ?? []
})
watch(verificationTags, (list, _previous, onCleanup) => {
  if (list.length) onCleanup(tags.subscribe(list))
})
watch(() => {
  if (!project.live || !verificationDevice.value?.online) return undefined
  for (const tag of verificationTags.value) {
    const change = tags.getLive(tag)
    if (change?.quality === 'good') return change
  }
  return undefined
}, (change) => {
  if (change) verifiedAt.value = change.tsMs
})

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

function stateOf(ch: ChannelDto): { label: string; tone: string } {
  return project.live ? channelStatus(project.channelStates[ch.id] ?? ch.state) : { label: '待确认', tone: 'idle' }
}

// —— 搜索 / 状态筛选：作用于设备卡片，滤空的总线整组隐藏 ——
const deviceFilter = ref('')
const statusFilter = ref<'all' | 'online' | 'offline'>('all')
const currentStatusFilter = computed(() => project.live ? statusFilter.value : 'all')
const channelFilter = computed(() =>
  route.query.add !== '1' && typeof route.query.channel === 'string' ? route.query.channel : '',
)
const listMode = ref<boolean>()
const compactList = computed(() => listMode.value ?? project.devices.length >= 8)
const STATUS_FILTERS = [
  { label: '全部', value: 'all' },
  { label: '在线', value: 'online' },
  { label: '离线', value: 'offline' },
] as const
watch(() => route.query.status, (status) => {
  statusFilter.value = status === 'online' || status === 'offline' ? status : 'all'
}, { immediate: true })

function matchesDevice(d: DeviceDto): boolean {
  const q = deviceFilter.value.trim().toLowerCase()
  const hit = !q || [d.id, d.name, addressLabel(d.address), project.packageOf(d)?.name]
    .some((field) => field?.toLowerCase().includes(q))
  const st =
    currentStatusFilter.value === 'all' ||
    (currentStatusFilter.value === 'online' ? d.online : !d.online)
  return hit && st
}

const filtering = computed(
  () => deviceFilter.value.trim() !== '' || currentStatusFilter.value !== 'all',
)

const deviceGroups = computed(() =>
  project.channels
    .filter((channel) => !channelFilter.value || channel.id === channelFilter.value)
    .map((channel) => ({ channel, devices: project.devicesOn(channel.id).filter(matchesDevice) }))
    .filter(({ devices }) => !filtering.value || devices.length > 0),
)
const visibleCount = computed(() => deviceGroups.value.reduce((count, group) => count + group.devices.length, 0))

function openWorkspace(d: DeviceDto): void {
  void router.push(`/d/${d.id}`)
}

function onlineOn(channelId: string): number {
  return project.devicesOn(channelId).filter((d) => d.online).length
}

function clearFilters(): void {
  deviceFilter.value = ''
  statusFilter.value = 'all'
  const { channel, status, ...query } = route.query
  if (channel || status) void router.replace({ query })
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
watch(compatiblePackages, () => {
  if (!compatiblePackages.value.some((p) => p.id === form.packageId)) {
    form.packageId = compatiblePackages.value[0]?.id || ''
  }
})

function openAdd(
  channelId?: string,
  address?: DeviceAddress,
  packageId?: string,
): void {
  const requestedPkg = project.packageById.get(packageId ?? '')
  const packageChannel = requestedPkg && project.channels.find((channel) =>
    packageSupportsTransport(requestedPkg, channel.transport, project.drivers),
  )
  form.channelId = channelId || packageChannel?.id || compatibleChannel.value?.id || project.channels[0]?.id || ''
  const allow = compatiblePackages.value
  form.packageId =
    (packageId && allow.some((p) => p.id === packageId) ? packageId : '') ||
    allow[0]?.id ||
    ''
  form.address = address !== undefined ? String(address) : '1'
  idTouched.value = false
  saveError.value = ''
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
    const n = parseAddress(form.address)
    if (typeof n !== 'number' || n < 1 || n > 247) return 'Modbus 从站地址需为 1–247 的整数'
  }
  return null
}

async function save(): Promise<void> {
  if (!project.live || saving.value) return
  const err = validate()
  if (err) {
    saveError.value = err
    return
  }
  saving.value = true
  saveError.value = ''
  const verify = firstDevice.value
  const id = form.id.trim()
  try {
    await project.addDevice({
      id,
      name: form.name.trim() || id,
      channelId: form.channelId,
      package: form.packageId,
      address: parseAddress(form.address),
    })
    toast.success(`设备 ${id} 已添加`)
    showForm.value = false
    if (verify) {
      verifiedAt.value = undefined
      verificationStartedAt.value = Date.now()
      verificationId.value = id
    }
  } catch (e) {
    saveError.value = errorMessage(e)
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

function addDeviceFromScan(address: DeviceAddress, packageId?: string): void {
  const ch = scanChannel.value
  scanChannel.value = null
  openAdd(ch?.id, address, packageId)
}

watch([() => project.live, () => route.query], ([live, q]) => {
  if (!live || route.path !== '/devices' || q.add !== '1') return
  if (compatibleChannel.value) {
    openAdd(
      typeof q.channel === 'string' ? q.channel : undefined,
      typeof q.address === 'string' ? q.address : undefined,
      typeof q.package === 'string' ? q.package : undefined,
    )
  }
  const { add, channel, address, package: packageId, ...query } = q
  void router.replace({ query })
}, { immediate: true })
</script>

<template>
  <div class="page devices-page">
    <PageHeader
      title="设备"
      subtitle="按采集通道管理设备，查看在线状态和点位"
    >
      <n-button v-if="!firstDevice" size="small" :disabled="!project.live" @click="openChannelCreate()">
        <template #icon><AppIcon name="plug" :size="15" /></template>
        新建通道
      </n-button>
      <n-button v-if="!firstDevice || compatibleChannel" size="small" @click="router.push('/packages')">
        <template #icon><AppIcon name="package" :size="15" /></template>
        设备包管理
      </n-button>
      <n-button
        v-if="!firstDevice"
        size="small"
        type="primary"
        :disabled="!project.live || !compatibleChannel"
        @click="openAdd(channelFilter || undefined)"
      >
        <template #icon><AppIcon name="plus" :size="15" /></template>
        添加设备
      </n-button>
    </PageHeader>

    <div v-if="!project.live" class="banner warn" role="status">
      <AppIcon name="alert" :size="16" />
      <span>{{ conn.connected ? '正在同步设备与通道。' : '网关连接已断开，设备和通道状态待确认。' }}</span>
      <n-button v-if="!conn.connected" size="tiny" quaternary @click="router.push('/diagnostics')">连接诊断</n-button>
    </div>

    <section v-if="firstDevice" class="card device-setup">
      <h3>接入第一台设备</h3>
      <ol class="setup-steps">
        <li :class="{ done: project.channels.length }">
          <AppIcon :name="project.channels.length ? 'check' : 'plug'" :size="16" /> 采集通道
        </li>
        <li :class="{ done: compatibleChannel }">
          <AppIcon :name="compatibleChannel ? 'check' : 'package'" :size="16" /> 兼容的设备包
        </li>
        <li><AppIcon name="activity" :size="16" /> 添加并验证设备</li>
      </ol>
      <p v-if="!project.channels.length">为现场设备创建串口、TCP 等采集通道，也可以先创建虚拟总线。</p>
      <p v-else-if="!compatibleChannel">{{ project.packages.length ? '现有设备包与通道不兼容，请导入支持该传输方式的型号。' : '导入与通道兼容的设备包，加载型号和点位定义。' }}</p>
      <p v-else>手填设备地址，或先扫描下方通道。添加后会验证能否收到有效读数。</p>
      <n-button v-if="!project.channels.length" type="primary" :disabled="!project.live" @click="openChannelCreate()">创建通道</n-button>
      <n-button v-else-if="!compatibleChannel" type="primary" :disabled="!project.live" @click="router.push('/packages?setup=1')">导入设备包</n-button>
      <n-button v-else type="primary" :disabled="!project.live" @click="openAdd()">添加并验证设备</n-button>
    </section>

    <section v-if="verificationDevice" class="card device-verification" aria-label="采集验证">
      <span class="group-icon"><AppIcon :name="verifiedAt === undefined ? 'activity' : 'check'" :size="19" /></span>
      <div class="group-identity" role="status">
        <h3>{{ verifiedAt !== undefined ? '已收到首个有效读数' : verificationWaiting ? '正在验证采集' : '暂未收到有效读数' }}</h3>
        <p class="muted small">{{ verificationDevice.name || verificationDevice.id }} ·
          {{ verifiedAt !== undefined ? formatClock(verifiedAt) : project.live ? '等待点值更新' : '等待网关恢复连接' }}
        </p>
      </div>
      <n-button size="small" @click="gotoTags(verificationDevice)">查看读数</n-button>
      <n-button size="small" quaternary @click="verificationId = ''">{{ verifiedAt === undefined ? '关闭' : '完成' }}</n-button>
    </section>

    <template v-if="project.channels.length">
      <div v-if="project.devices.length || channelFilter" class="toolbar">
        <div class="search-box">
          <AppIcon name="search" :size="14" class="search-icon" />
          <input
            v-model="deviceFilter"
            class="search-input"
            type="search"
            placeholder="搜索设备名称 / ID"
            aria-label="搜索设备"
          />
        </div>
        <div class="filter-chips">
          <button
            v-for="f in STATUS_FILTERS"
            :key="f.value"
            type="button"
            class="filter-chip"
            :class="{ active: currentStatusFilter === f.value }"
            :aria-pressed="currentStatusFilter === f.value"
            :disabled="!project.live && f.value !== 'all'"
            @click="statusFilter = f.value"
          >
            {{ f.label }}
          </button>
        </div>
        <n-button v-if="channelFilter" size="tiny" quaternary @click="clearFilters">显示全部通道</n-button>
        <span class="spacer" />
        <span class="muted">
          {{ filtering || channelFilter ? `匹配 ${visibleCount} 台` : `${project.channels.length} 条通道 · ${project.devices.length} 台设备` }}
        </span>
        <div v-if="project.devices.length" class="filter-chips" aria-label="设备视图">
          <button type="button" class="filter-chip" :class="{ active: !compactList }" :aria-pressed="!compactList" @click="listMode = false">卡片</button>
          <button type="button" class="filter-chip" :class="{ active: compactList }" :aria-pressed="compactList" @click="listMode = true">列表</button>
        </div>
      </div>

      <section v-for="{ channel: ch, devices } in deviceGroups" :key="ch.id" class="channel-group">
        <header class="group-head">
          <span class="group-icon"><AppIcon name="network" :size="19" /></span>
          <div class="group-identity">
            <h3>{{ ch.name || ch.id }}</h3>
            <small>{{ transportLabel(ch.transport) }}<template v-if="ch.transport !== 'sim'"> · {{ channelSummary(ch) }}</template></small>
          </div>
          <span class="state-pill" :class="stateOf(ch).tone">{{ stateOf(ch).label }}</span>
          <span v-if="ch.configError" class="state-pill bad" :title="ch.configError">配置错误</span>
          <div class="spacer" />
          <span class="muted small">
            {{ project.devicesOn(ch.id).length }} 台<template v-if="project.live"> · 在线 {{ onlineOn(ch.id) }}</template>
          </span>
          <n-button size="tiny" quaternary :disabled="!project.live" @click="scanChannel = ch">
            <template #icon><AppIcon name="scan" :size="14" /></template>
            扫描
          </n-button>
          <n-button size="tiny" quaternary :disabled="!project.live" @click="openChannelEdit(ch)">编辑</n-button>
          <n-button v-if="!firstDevice" size="tiny" quaternary :disabled="!project.live" @click="openAdd(ch.id)">
            <template #icon><AppIcon name="plus" :size="14" /></template>
            添加设备
          </n-button>
        </header>

        <div v-if="devices.length" class="device-grid" :class="{ 'device-grid-list': compactList }">
          <article v-for="d in devices" :key="d.id" class="card device-card">
            <div class="device-top">
              <span class="device-glyph"><AppIcon name="chip" :size="18" /></span>
              <button
                type="button"
                class="device-title"
                :title="`打开 ${d.name || d.id} 工作区`"
                @click="openWorkspace(d)"
              >
                <strong>{{ d.name || d.id }}</strong>
                <code>{{ d.id }}</code>
              </button>
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
                <span class="mono">{{ pointCount(project.packageOf(d)?.points) }}</span>
              </div>
              <div>
                <span class="label">状态</span>
                <span class="state-pill" :class="!project.live ? 'idle' : d.online ? 'ok' : 'warn'">
                  {{ project.live ? (d.online ? '在线' : '离线') : '待确认' }}
                </span>
              </div>
            </div>

            <div v-if="d.configError" class="banner warn mt-8 small">
              <AppIcon name="alert" :size="14" />
              <span>{{ d.configError }}</span>
            </div>

            <div class="device-foot">
              <n-button size="tiny" @click="openWorkspace(d)">
                <template #icon><AppIcon name="layout" :size="14" /></template>
                打开工作区
              </n-button>
              <n-button size="tiny" quaternary @click="gotoTags(d)">
                <template #icon><AppIcon name="activity" :size="14" /></template>
                数据浏览
              </n-button>
              <div class="spacer" />
              <n-button size="tiny" quaternary type="error" :disabled="!project.live" @click="pendingDelete = d">删除</n-button>
            </div>
          </article>

        </div>

        <div v-if="!project.devicesOn(ch.id).length" class="group-empty muted">
          该通道还没有设备。
        </div>
      </section>

      <div v-if="!deviceGroups.length" class="banner info">
        <AppIcon name="search" :size="16" />
        <span>没有符合当前搜索 / 筛选条件的设备。</span>
        <n-button size="tiny" quaternary @click="clearFilters">
          清除筛选
        </n-button>
      </div>
    </template>

    <EmptyState
      v-else-if="!project.loaded"
      icon="network"
      title="等待设备清单"
    />

    <SerialPortsCard v-if="project.live" @use="openChannelCreate" />

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
    <ModalShell v-if="showForm" title="添加设备" subtitle="设备 ID 是 tag 前缀，添加后不可与已有设备重复" :width="560" @close="!saving && (showForm = false)">
      <div v-if="!compatiblePackages.length" class="banner warn mb-12">
        <AppIcon name="alert" :size="16" />
        <span>
          当前通道没有兼容的设备包。
        </span>
        <n-button size="tiny" quaternary @click="router.push('/packages')">前往设备包</n-button>
      </div>
      <div class="form-grid">
        <div class="field span-2">
          <label>通道</label>
          <n-select v-model:value="form.channelId" :options="channelOptions" placeholder="选择采集通道" />
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
      <p v-if="saveError" class="banner danger mt-12" role="alert">{{ saveError }}</p>
      <template #footer>
        <n-button size="small" :disabled="saving" @click="showForm = false">取消</n-button>
        <n-button size="small" type="primary" :loading="saving" :disabled="!project.live || !form.packageId" @click="save">
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
