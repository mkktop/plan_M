<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'
import { NButton, NInput } from 'naive-ui'
import PageHeader from '@/components/PageHeader.vue'
import AppIcon from '@/components/AppIcon.vue'
import ModalShell from '@/components/ModalShell.vue'
import { gateway, isTauriRuntime } from '@/api/ws'
import { useConnectionStore } from '@/stores/connection'
import { useToastStore } from '@/stores/toast'
import { useUpdatesStore } from '@/stores/updates'

const conn = useConnectionStore()
const toast = useToastStore()
const updates = useUpdatesStore()

// 模板里统一走这层别名（Volar 对 class 实例上的 reactive 字段做 ref 解包时类型会跑偏）。
const gs = gateway.state

// —— 数据目录（仅桌面版：查看/迁移）——

interface StorageInfo {
  dataDir: string
  isDefault: boolean
  defaultDataDir: string
  configPath: string
  gatewaydManaged: boolean
}

const storage = ref<StorageInfo | null>(null)
const storageLoading = ref(false)
const migrating = ref(false)
const migrateTarget = ref('')
const migrateConfirmOpen = ref(false)

const canMigrate = computed(
  () => isTauriRuntime() && storage.value !== null && storage.value.gatewaydManaged,
)

async function loadStorage(): Promise<void> {
  if (!isTauriRuntime()) return
  storageLoading.value = true
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    storage.value = await invoke<StorageInfo>('get_storage_info')
  } catch (e) {
    // 展示层失败不阻断设置页
    storage.value = null
    console.warn('读取存储信息失败', e)
  } finally {
    storageLoading.value = false
  }
}

async function pickDataDir(): Promise<void> {
  if (!canMigrate.value) return
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const picked = await open({
      multiple: false,
      directory: true,
      title: '选择新的数据目录（必须为空）',
    })
    if (typeof picked === 'string' && picked) {
      migrateTarget.value = picked
      migrateConfirmOpen.value = true
    }
  } catch (e) {
    toast.error(e instanceof Error ? e.message : String(e))
  }
}

async function confirmMigrate(): Promise<void> {
  if (!migrateTarget.value) return
  migrating.value = true
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    storage.value = await invoke<StorageInfo>('migrate_data_dir', {
      newDir: migrateTarget.value,
    })
    migrateConfirmOpen.value = false
    toast.success('数据目录已迁移，原目录保留作备份')
    // gatewayd 已在新目录重启；重连后快照/订阅自动恢复
    await conn.reconnect()
  } catch (e) {
    toast.error(e instanceof Error ? e.message : String(e))
  } finally {
    migrating.value = false
  }
}

// —— 连接设置（从诊断页迁移过来）——

const showToken = ref(false)
const applying = ref(false)
const form = reactive({ wsUrl: gs.settings.wsUrl, token: gs.settings.token })

// Volar 坑：ws:// 字面量不能放模板静态属性（会破坏后续表达式的类型解包），改绑表达式。
const WS_PLACEHOLDER = 'ws://127.0.0.1:59120/ws'

const tokenMasked = computed(() => {
  const t = gs.settings.token
  if (!t) return '未配置'
  return `${t.slice(0, 4)}••••${t.slice(-4)}`
})

async function applySettings(): Promise<void> {
  const wsUrl = form.wsUrl.trim()
  if (!wsUrl) {
    toast.error('请填写 gatewayd 的 WebSocket 地址')
    return
  }
  applying.value = true
  try {
    await gateway.start({ wsUrl, token: form.token.trim() })
    toast.success('已应用连接设置')
  } finally {
    applying.value = false
  }
}

async function reconnect(): Promise<void> {
  await conn.reconnect()
  toast.info('正在重新连接 gatewayd')
}

// —— 应用与更新 ——

const isDesktop = isTauriRuntime()
const versionText = computed(() => {
  const app = updates.currentVersion || gs.hostVersion || gs.info?.hostVersion || ''
  const host = gs.hostVersion
  if (!app) return '—'
  if (host && app !== host) return `桌面 v${app} · gatewayd v${host}`
  return `v${app}`
})

const publishedAtText = computed(() => {
  const raw = updates.result?.published_at
  if (!raw) return ''
  const d = new Date(raw)
  return Number.isNaN(d.getTime()) ? raw : d.toLocaleString()
})

const notesHtml = computed(() => {
  const raw = (updates.result?.notes ?? '').trim()
  return raw
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .slice(0, 30)
    .map((line) => `<p>${line.replace(/[<>&]/g, (c) => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;' })[c] ?? c)}</p>`)
    .join('')
})

const downloadSizeText = computed(() => {
  const n = updates.result?.download_size
  if (!n) return ''
  return n >= 1024 * 1024 ? `${(n / 1024 / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`
})

onMounted(() => {
  void updates.loadVersion()
  void loadStorage()
})
</script>

<template>
  <div class="page settings-page">
    <PageHeader title="设置" subtitle="连接参数与应用更新" />

    <section class="card settings-card">
      <div class="card-head">
        <h3>连接</h3>
        <span class="muted small">gatewayd 采集服务的 WebSocket 地址与访问 token</span>
      </div>

      <div class="settings-form">
        <div class="field">
          <label>WebSocket 地址</label>
          <n-input v-model:value="form.wsUrl" :placeholder="WS_PLACEHOLDER" />
        </div>
        <div class="field">
          <label>访问 token</label>
          <n-input
            v-model:value="form.token"
            :type="showToken ? 'text' : 'password'"
            placeholder="gatewayd 数据目录下的 gateway.token"
          >
            <template #suffix>
              <button type="button" class="input-icon" title="显示/隐藏" @click="showToken = !showToken">
                <AppIcon name="eye" :size="15" />
              </button>
            </template>
          </n-input>
          <p class="hint">
            桌面壳会自动读取 <code>%APPDATA%/gatewayd/gateway.token</code>；浏览器调试可带
            <code>?token=…</code> 打开。
          </p>
        </div>
        <div class="form-actions">
          <n-button size="small" :loading="applying" type="primary" @click="applySettings">
            <template #icon><AppIcon name="check" :size="15" /></template>
            保存并连接
          </n-button>
          <n-button size="small" :disabled="!conn.connected" @click="reconnect">
            <template #icon><AppIcon name="refresh" :size="15" /></template>
            重新连接
          </n-button>
        </div>
      </div>

      <dl class="kv-list">
        <div class="kv">
          <dt>连接状态</dt>
          <dd>
            <span class="status-orb" :class="conn.state.status" />
            {{ conn.statusLabel }}
          </dd>
        </div>
        <div class="kv">
          <dt>Token</dt>
          <dd class="mono">{{ tokenMasked }}</dd>
        </div>
        <div v-if="gs.info" class="kv">
          <dt>数据目录</dt>
          <dd class="mono ellipsis" :title="gs.info.dataDir">{{ gs.info.dataDir }}</dd>
        </div>
        <div v-if="gs.info" class="kv">
          <dt>Token 文件</dt>
          <dd class="mono ellipsis" :title="gs.info.tokenPath">{{ gs.info.tokenPath }}</dd>
        </div>
      </dl>

      <div v-if="!gs.settings.token" class="banner warn">
        <AppIcon name="alert" :size="16" />
        <span>尚未配置 token。启动 <code>gatewayd serve</code> 后从数据目录读取 token 填入。</span>
      </div>
    </section>

    <section class="card settings-card">
      <div class="card-head">
        <h3>数据目录</h3>
        <span class="muted small">通道 / 设备 / 仪表盘 / 设备包 / 插件存放的位置</span>
      </div>

      <template v-if="isDesktop">
        <dl class="kv-list">
          <div class="kv">
            <dt>当前目录</dt>
            <dd class="mono ellipsis" :title="storage?.dataDir">
              {{ storage?.dataDir || gs.info?.dataDir || '…' }}
            </dd>
          </div>
          <div class="kv">
            <dt>位置</dt>
            <dd>
              <span v-if="!storage" class="muted">{{ storageLoading ? '读取中…' : '未知' }}</span>
              <span v-else-if="storage.isDefault" class="muted">默认（未迁移过）</span>
              <span v-else class="text-good">已自定义</span>
            </dd>
          </div>
          <div v-if="storage" class="kv">
            <dt>设置文件</dt>
            <dd class="mono ellipsis" :title="storage.configPath">{{ storage.configPath }}</dd>
          </div>
        </dl>

        <div class="form-actions">
          <n-button size="small" :loading="migrating" :disabled="!canMigrate" @click="pickDataDir">
            <template #icon><AppIcon name="folder" :size="15" /></template>
            迁移数据目录…
          </n-button>
        </div>
        <p v-if="storage && !storage.gatewaydManaged" class="hint">
          当前 gatewayd 不是本应用拉起的（外部实例或未运行），迁移前请先关闭它并重开应用。
        </p>
        <p class="hint">
          迁移会复制全部数据（不含运行日志）到新目录并重启采集服务；<strong>原目录保留作备份</strong>，
          目标目录必须为空。
        </p>
      </template>
      <p v-else class="hint mb-0">浏览器调试模式不支持迁移数据目录；当前目录见「连接」卡片。</p>
    </section>

    <section class="card settings-card">
      <div class="card-head">
        <h3>应用与更新</h3>
        <span class="muted small">从 GitHub Releases 检查，下载安装包并自动校验摘要</span>
      </div>

      <div class="update-hero">
        <div>
          <div class="update-version mono">{{ versionText }}</div>
          <div class="muted small">
            当前版本{{ isDesktop ? '' : ' · 浏览器调试模式不支持应用内更新' }}
          </div>
        </div>
        <div class="update-actions">
          <n-button
            size="small"
            :loading="updates.phase === 'checking'"
            :disabled="updates.busy || !isDesktop"
            @click="updates.check()"
          >
            <template #icon><AppIcon name="refresh" :size="15" /></template>
            检查更新
          </n-button>
          <n-button size="small" quaternary :disabled="updates.busy" @click="updates.openRelease()">
            <template #icon><AppIcon name="link" :size="15" /></template>
            打开发布页
          </n-button>
        </div>
      </div>

      <div v-if="updates.message" class="banner" :class="updates.available ? 'warn' : 'info'">
        <AppIcon :name="updates.available ? 'download' : 'check'" :size="16" />
        <span>{{ updates.message }}</span>
        <span v-if="updates.available && updates.result?.can_install && downloadSizeText" class="muted small">
          · {{ downloadSizeText }}
        </span>
        <span v-if="publishedAtText" class="muted small">· {{ publishedAtText }}</span>
      </div>
      <div v-if="updates.error" class="banner danger">
        <AppIcon name="alert" :size="16" />
        <span>{{ updates.error }}</span>
      </div>

      <!-- 有新版本但资产无法校验时（can_install=false）只引导去发布页 -->
      <div v-if="updates.available && !updates.result?.can_install && !updates.error" class="banner warn">
        <AppIcon name="alert" :size="16" />
        <span>该版本安装包暂不支持应用内安装，请打开发布页手动下载。</span>
      </div>

      <div v-if="notesHtml && updates.available" class="update-notes">
        <div class="update-notes-head muted small">更新说明</div>
        <!-- eslint-disable-next-line vue/no-v-html —— 内容已做 HTML 转义，仅换行成段 -->
        <div class="update-notes-body" v-html="notesHtml" />
      </div>

      <div v-if="updates.result?.can_install" class="update-install">
        <div
          v-if="['downloading', 'ready', 'installing'].includes(updates.phase)"
          class="update-progress"
        >
          <div class="update-progress-bar">
            <div class="update-progress-fill" :style="{ width: `${updates.percent}%` }" />
          </div>
          <span class="muted small mono">
            {{ updates.phase === 'ready' || updates.phase === 'installing' ? '100%' : `${updates.percent}%` }}
          </span>
        </div>
        <div class="form-actions">
          <n-button
            v-if="updates.phase === 'ready' || updates.phase === 'installing'"
            size="small"
            type="primary"
            :loading="updates.phase === 'installing'"
            :disabled="updates.phase === 'installing'"
            @click="updates.install()"
          >
            <template #icon><AppIcon name="power" :size="15" /></template>
            安装并重启
          </n-button>
          <n-button
            v-else
            size="small"
            type="primary"
            :loading="updates.phase === 'downloading'"
            :disabled="updates.busy"
            @click="updates.download()"
          >
            <template #icon><AppIcon name="download" :size="15" /></template>
            下载新版本
          </n-button>
        </div>
        <p v-if="updates.phase === 'ready'" class="hint">
          安装包已通过 sha256 校验；点击后拉起安装程序并退出应用（gatewayd 采集不受影响）。
        </p>
      </div>
    </section>

    <ModalShell
      v-if="migrateConfirmOpen"
      title="迁移数据目录"
      subtitle="复制数据并重启采集服务"
      :width="560"
      @close="migrateConfirmOpen = false"
    >
      <div class="migrate-paths">
        <div class="migrate-path">
          <span class="muted small">当前</span>
          <code class="mono">{{ storage?.dataDir }}</code>
        </div>
        <div class="migrate-path">
          <span class="muted small">迁移到</span>
          <code class="mono">{{ migrateTarget }}</code>
        </div>
      </div>
      <ul class="migrate-notes muted small">
        <li>复制通道 / 设备 / 仪表盘 / 工作区 / 设备包 / 插件（不含运行日志）</li>
        <li>迁移期间采集服务会短暂重启，页面自动重连</li>
        <li>原目录原样保留作备份，确认无误后可自行删除</li>
      </ul>
      <template #footer>
        <n-button size="small" :disabled="migrating" @click="migrateConfirmOpen = false">取消</n-button>
        <n-button size="small" type="primary" :loading="migrating" @click="confirmMigrate">
          开始迁移
        </n-button>
      </template>
    </ModalShell>
  </div>
</template>
