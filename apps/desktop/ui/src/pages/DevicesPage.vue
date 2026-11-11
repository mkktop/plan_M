<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NButton, NInput, NInputNumber, NSelect } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import EmptyState from '@/components/EmptyState.vue'
import ModalShell from '@/components/ModalShell.vue'
import PageHeader from '@/components/PageHeader.vue'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { errorMessage, useToastStore } from '@/stores/toast'
import type { ChannelDto, DeviceDto } from '@/api/types'

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

const form = reactive({
  channelId: '',
  packageId: '',
  slave: 1,
  id: '',
  name: '',
})

const importOpen = ref(false)
const importPath = ref('')
const importing = ref(false)

const channelOptions = computed(() =>
  project.channels.map((c) => ({ label: `${c.name || c.id}（${c.port}）`, value: c.id })),
)
const packageOptions = computed(() =>
  project.packages.map((p) => ({ label: `${p.name} · v${p.version}`, value: p.id })),
)

const STATE_TONE: Record<string, string> = {
  running: 'ok',
  recovering: 'warn',
  faulted: 'bad',
  stopped: 'idle',
}

function stateTone(ch: ChannelDto): string {
  return STATE_TONE[project.channelStates[ch.id] ?? ch.state] ?? 'idle'
}

function pointCount(d: DeviceDto): number {
  const pkg = project.packageOf(d)
  return pkg ? pkg.points.reduce((n, p) => n + 1 + p.bits.length, 0) : 0
}

function autoFill(): void {
  if (idTouched.value) return
  const pkg = project.packageById.get(form.packageId)
  const slug = pkg?.id.split('.').filter(Boolean).pop() || 'dev'
  const base = `${slug}-${form.slave}`
  let id = base
  let n = 2
  while (project.devices.some((d) => d.id === id)) id = `${base}-${n++}`
  form.id = id
  form.name = pkg ? `${pkg.name} #${form.slave}` : `设备 #${form.slave}`
}

watch([() => form.packageId, () => form.slave], autoFill)

function openAdd(channelId?: string, slave?: number): void {
  form.channelId = channelId || project.channels[0]?.id || ''
  form.packageId = project.packages[0]?.id || ''
  form.slave = slave && slave > 0 ? slave : 1
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
  if (form.slave < 1 || form.slave > 247) return '从站地址需在 1–247'
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
      slave: form.slave,
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

async function doImport(): Promise<void> {
  const path = importPath.value.trim()
  if (!path) {
    toast.error('请填写设备包目录或 zip 的绝对路径')
    return
  }
  importing.value = true
  try {
    await project.importPackage(path)
    toast.success('设备包已导入')
    importOpen.value = false
    importPath.value = ''
  } catch (e) {
    toast.error(errorMessage(e))
  } finally {
    importing.value = false
  }
}

function gotoTags(d: DeviceDto): void {
  router.push({ path: '/tags', query: { device: d.id } })
}

// 从「扫描从站」跳转过来时自动打开添加对话框（快照可能已加载，也可能还在路上）。
const prefill = ref<{ channel?: string; slave?: number } | null>(null)

function applyPrefill(): void {
  if (!project.loaded || !prefill.value) return
  const p = prefill.value
  prefill.value = null
  openAdd(p.channel, p.slave)
}

watch(() => project.loaded, applyPrefill, { immediate: true })
watch(prefill, applyPrefill)

onMounted(() => {
  const q = route.query
  if (q.add === '1') {
    prefill.value = {
      channel: typeof q.channel === 'string' ? q.channel : undefined,
      slave: Number(q.slave) || 1,
    }
    void router.replace({ query: {} })
  }
})
</script>

<template>
  <div class="page">
    <PageHeader title="设备" subtitle="设备 = 设备包 + 通道 + 从站地址；导入设备包即可支持新型号">
      <n-button size="small" @click="importOpen = true">
        <template #icon><AppIcon name="download" :size="15" /></template>
        导入设备包
      </n-button>
      <n-button size="small" type="primary" :disabled="!project.channels.length || !project.packages.length" @click="openAdd()">
        <template #icon><AppIcon name="plus" :size="15" /></template>
        添加设备
      </n-button>
    </PageHeader>

    <div v-if="!conn.connected" class="banner warn">
      <AppIcon name="alert" :size="16" />
      <span>未连接 gatewayd，设备配置不可编辑。</span>
    </div>

    <div v-if="!project.channels.length && project.loaded" class="banner warn">
      <AppIcon name="alert" :size="16" />
      <span>还没有通道。先到「通道」页创建一条串口通道，再添加设备。</span>
      <n-button size="tiny" quaternary @click="router.push('/channels')">去创建</n-button>
    </div>

    <div v-if="!project.packages.length && project.loaded" class="banner info">
      <AppIcon name="alert" :size="16" />
      <span>没有已安装的设备包。导入 <code>manifest.json + points.json</code> 的目录或 zip。</span>
    </div>

    <template v-if="project.devices.length">
      <section v-for="ch in project.channels" :key="ch.id" class="channel-group">
        <header class="group-head">
          <span class="state-dot" :class="stateTone(ch)" />
          <h3>{{ ch.name || ch.id }}</h3>
          <code>{{ ch.id }} · {{ ch.port }}</code>
          <span class="muted small">{{ project.devicesOn(ch.id).length }} 台</span>
          <div class="spacer" />
          <n-button size="tiny" quaternary @click="openAdd(ch.id)">
            <template #icon><AppIcon name="plus" :size="14" /></template>
            添加
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
              <span class="slave-badge mono">#{{ d.slave }}</span>
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
          该通道还没有设备
        </div>
      </section>
    </template>

    <EmptyState
      v-else-if="project.loaded"
      icon="chip"
      title="还没有设备"
      text="选择通道、设备包与从站地址即可添加；同型号多台设备共用一份点表。"
    >
      <n-button size="small" type="primary" :disabled="!project.channels.length || !project.packages.length" @click="openAdd()">
        添加设备
      </n-button>
    </EmptyState>

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
          <label>从站地址</label>
          <n-input-number v-model:value="form.slave" :min="1" :max="247" placeholder="从站地址" style="width: 100%" />
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

    <!-- 导入设备包 -->
    <ModalShell v-if="importOpen" title="导入设备包" subtitle="目录或 zip：manifest.json + points.json，导入即注册新型号" :width="560" @close="importOpen = false">
      <div class="field">
        <label>设备包路径</label>
        <n-input v-model:value="importPath" placeholder="D:\\packages\\acme_pm3000.zip" />
        <p class="hint">
          路径是 gatewayd 所在机器的绝对路径。校验失败会返回具体字段原因；成功后可立即在设备页选用。
        </p>
      </div>
      <template #footer>
        <n-button size="small" @click="importOpen = false">取消</n-button>
        <n-button size="small" type="primary" :loading="importing" @click="doImport">导入</n-button>
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
  </div>
</template>
