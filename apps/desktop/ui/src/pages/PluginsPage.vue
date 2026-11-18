<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { NButton, NInput } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import EmptyState from '@/components/EmptyState.vue'
import ModalShell from '@/components/ModalShell.vue'
import PageHeader from '@/components/PageHeader.vue'
import { isTauriRuntime } from '@/api/ws'
import { useConnectionStore } from '@/stores/connection'
import { usePluginsStore } from '@/stores/plugins'
import { errorMessage, useToastStore } from '@/stores/toast'
import { pluginHost, type PluginStatus } from '@/plugins/host'
import type { UiPluginDto } from '@/api/types'

const router = useRouter()
const plugins = usePluginsStore()
const conn = useConnectionStore()
const toast = useToastStore()

const importOpen = ref(false)
const importPath = ref('')
const importing = ref(false)
const importError = ref('')
const dragOver = ref(false)
const removing = ref<UiPluginDto | null>(null)
const removeBusy = ref(false)
const pluginFilter = ref('')

const PERMISSION_LABEL: Record<string, string> = {
  'tags:subscribe': '订阅点位',
  'tags:write': '写点',
  notify: '通知',
  theme: '主题',
}

const readyCount = computed(() => pluginHost.statuses.filter((s) => !s.error).length)
const filteredPlugins = computed(() => {
  const query = pluginFilter.value.trim().toLowerCase()
  return plugins.list.filter((plugin) =>
    [plugin.name, plugin.id, plugin.description].some((field) => field?.toLowerCase().includes(query)),
  )
})

function statusOf(id: string): PluginStatus | undefined {
  return pluginHost.statuses.find((s) => s.id === id)
}

async function refresh(force = false): Promise<void> {
  if (!conn.connected) return
  try {
    await plugins.refresh(force)
  } catch (e) {
    toast.error(`读取插件列表失败：${errorMessage(e)}`)
  }
}

watch(
  () => conn.connected,
  (ok) => {
    if (ok) void refresh()
  },
  { immediate: true },
)

function openImport(): void {
  importError.value = ''
  importOpen.value = true
}

async function doImport(): Promise<void> {
  const path = importPath.value.trim()
  if (!path) {
    importError.value = '请填写插件目录或 zip 的绝对路径'
    return
  }
  importing.value = true
  importError.value = ''
  const before = new Map(plugins.list.map((p) => [p.id, p.version]))
  try {
    const list = await plugins.importPlugin(path)
    const changed =
      list.find((p) => !before.has(p.id)) ??
      list.find((p) => before.get(p.id) !== p.version) ??
      list[0]
    toast.success(changed ? `插件 ${changed.name} v${changed.version} 已安装` : '插件已导入')
    importOpen.value = false
    importPath.value = ''
  } catch (e) {
    importError.value = errorMessage(e)
  } finally {
    importing.value = false
  }
}

async function pickSource(directory: boolean): Promise<void> {
  if (!isTauriRuntime()) {
    toast.info('浏览器调试模式无法读取本地路径，请在下方填写 gatewayd 本机绝对路径')
    return
  }
  try {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const picked = await open({
      multiple: false,
      directory,
      title: directory ? '选择插件目录' : '选择插件 zip',
      filters: directory ? undefined : [{ name: 'UI 插件', extensions: ['zip'] }],
    })
    if (typeof picked === 'string' && picked) {
      importPath.value = picked
      await doImport()
    }
  } catch (e) {
    importError.value = errorMessage(e)
  }
}

function onHtmlDrop(e: DragEvent): void {
  dragOver.value = false
  if (isTauriRuntime()) return // 桌面端由 Tauri 原生拖放事件接管
  e.preventDefault()
  toast.info('浏览器调试模式无法读取拖入文件的路径，请填写 gatewayd 本机绝对路径')
}

let unlistenDrag: (() => void) | undefined

onMounted(async () => {
  if (!isTauriRuntime()) return
  try {
    const { getCurrentWebview } = await import('@tauri-apps/api/webview')
    unlistenDrag = await getCurrentWebview().onDragDropEvent((event) => {
      if (!importOpen.value) return
      const p = event.payload
      if (p.type === 'enter' || p.type === 'over') {
        dragOver.value = true
      } else if (p.type === 'leave') {
        dragOver.value = false
      } else if (p.type === 'drop') {
        dragOver.value = false
        const path = p.paths[0]
        if (!path) return
        importPath.value = path
        void doImport()
      }
    })
  } catch (e) {
    console.warn('注册拖放事件失败', e)
  }
})

onBeforeUnmount(() => {
  unlistenDrag?.()
})

async function doRemove(): Promise<void> {
  const p = removing.value
  if (!p) return
  removeBusy.value = true
  try {
    await plugins.remove(p.id)
    toast.success(`插件 ${p.name} 已移除`)
    removing.value = null
  } catch (e) {
    toast.error(`移除失败：${errorMessage(e)}`)
  } finally {
    removeBusy.value = false
  }
}
</script>

<template>
  <div class="page">
    <PageHeader
      title="插件"
      subtitle="管理工作台的扩展组件、页面和插件权限"
    >
      <n-button size="small" :disabled="!conn.connected" :loading="plugins.loading" @click="refresh(true)">
        <template #icon><AppIcon name="refresh" :size="15" /></template>
        刷新
      </n-button>
      <n-button size="small" type="primary" :disabled="!conn.connected" @click="openImport">
        <template #icon><AppIcon name="download" :size="15" /></template>
        导入插件
      </n-button>
    </PageHeader>

    <div v-if="!conn.connected" class="banner warn">
      <AppIcon name="alert" :size="16" />
      <span>未连接 gatewayd，无法导入或查看 UI 插件。</span>
    </div>

    <div v-if="plugins.list.length" class="card plugin-summary">
      <div class="plugin-search">
        <n-input v-model:value="pluginFilter" clearable placeholder="搜索插件" aria-label="搜索插件">
          <template #prefix><AppIcon name="search" :size="15" /></template>
        </n-input>
      </div>
      <span class="muted">已安装 {{ plugins.list.length }}</span>
      <span class="state-pill ok">{{ readyCount }} 个已加载</span>
    </div>

    <div v-if="filteredPlugins.length" class="plugin-grid">
      <article v-for="p in filteredPlugins" :key="p.id" class="card plugin-card">
        <header class="plugin-card-head">
          <span class="plugin-glyph"><AppIcon name="plugin" :size="19" /></span>
          <div class="plugin-card-title">
            <strong>{{ p.name }}</strong>
            <code>{{ p.id }}</code>
          </div>
          <span class="mono plugin-version">v{{ p.version }}</span>
        </header>

        <p v-if="p.description" class="plugin-desc">{{ p.description }}</p>
        <p v-else class="plugin-desc muted">未提供描述</p>

        <div class="plugin-chips">
          <span v-for="perm in p.permissions" :key="perm" class="perm-chip" :title="perm">
            {{ PERMISSION_LABEL[perm] ?? perm }}
          </span>
          <span v-if="!p.permissions.length" class="perm-chip muted">无权限</span>
        </div>

        <div class="plugin-stats">
          <span><b>{{ p.widgets.length }}</b> 个仪表盘组件</span>
          <span><b>{{ p.pages.length }}</b> 个导航页</span>
          <span class="mono">SDK {{ p.sdkVersion }}</span>
        </div>

        <div v-if="statusOf(p.id)?.error" class="plugin-error">
          <AppIcon name="alert" :size="14" />
          <span>{{ statusOf(p.id)?.error }}</span>
        </div>
        <div v-else-if="statusOf(p.id)" class="plugin-ok">
          <span class="online-dot on" />
          <span>入口已加载</span>
          <code class="muted">{{ p.entry }}</code>
        </div>
        <div v-else class="muted small">等待加载</div>

        <footer class="plugin-card-foot">
          <n-button
            v-for="pg in p.pages"
            :key="pg.id"
            size="tiny"
            quaternary
            @click="router.push(`/plugin/${p.id}/${pg.id}`)"
          >
            <template #icon><AppIcon :name="pg.icon || 'plugin'" :size="13" /></template>
            {{ pg.title }}
          </n-button>
          <span class="spacer" />
          <n-button size="tiny" quaternary type="error" @click="removing = p">
            <template #icon><AppIcon name="trash" :size="13" /></template>
            移除
          </n-button>
        </footer>
      </article>
    </div>

    <EmptyState v-else-if="plugins.list.length" icon="search" title="没有匹配的插件">
      <n-button size="small" @click="pluginFilter = ''">清除搜索</n-button>
    </EmptyState>
    <EmptyState
      v-else-if="conn.connected && plugins.loaded"
      icon="plugin"
      title="还没有安装 UI 插件"
      text="导入插件包以添加仪表盘组件和导航页面。"
    >
      <n-button size="small" type="primary" @click="openImport">导入插件</n-button>
    </EmptyState>

    <details class="card plugin-doc">
      <summary>插件开发说明</summary>
      <pre class="mono">plugin/
├─ manifest.json   # id/version/sdkVersion/permissions + widgets/pages
└─ index.js        # export function mount(el, ctx, props) { ... }
</pre>
      <p class="muted">
        仪表盘组件 kind 为 <code>plugin:&lt;插件id&gt;:&lt;组件id&gt;</code>；插件不直接开 WebSocket，
        <code>ctx.tags.subscribe/read/write</code> 由宿主聚合与鉴权。示例插件见
        <code>examples/ui-plugins/meter-card</code>。
      </p>
    </details>

    <!-- 导入插件 -->
    <ModalShell
      v-if="importOpen"
      title="导入 UI 插件"
      subtitle="目录或 zip；gatewayd 校验 manifest、entry 与权限后安装到 ui-plugins/"
      :width="620"
      @close="importOpen = false"
    >
      <div
        class="drop-zone"
        :class="{ over: dragOver }"
        @dragover.prevent="dragOver = true"
        @dragleave="dragOver = false"
        @drop="onHtmlDrop"
      >
        <div class="drop-icon"><AppIcon name="plugin" :size="24" /></div>
        <strong>把插件目录或 zip 拖到这里</strong>
        <p class="muted small">
          桌面壳支持原生拖放；也可以选择目录/文件，或在下方填写 gatewayd 本机绝对路径。
        </p>
        <div class="drop-actions">
          <n-button size="small" @click="pickSource(false)">
            <template #icon><AppIcon name="download" :size="14" /></template>
            选择 zip
          </n-button>
          <n-button size="small" @click="pickSource(true)">
            <template #icon><AppIcon name="folder" :size="14" /></template>
            选择目录
          </n-button>
        </div>
      </div>

      <div class="field mt-14">
        <label>插件路径（gatewayd 本机绝对路径）</label>
        <n-input
          v-model:value="importPath"
          placeholder="例如 D:\\work\\plugins\\meter-card 或 meter-card.zip"
          @keyup.enter="doImport"
        />
      </div>

      <pre v-if="importError" class="import-error">{{ importError }}</pre>

      <template #footer>
        <n-button size="small" @click="importOpen = false">取消</n-button>
        <n-button size="small" type="primary" :loading="importing" @click="doImport">
          导入
        </n-button>
      </template>
    </ModalShell>

    <!-- 移除插件 -->
    <ModalShell
      v-if="removing"
      title="移除 UI 插件"
      :width="460"
      @close="removing = null"
    >
      <p>确认移除插件 <b>{{ removing.name }}</b>（{{ removing.id }}）？</p>
      <p class="muted mt-8">
        已放在仪表盘里的插件组件会退化为占位符；插件目录会从 gatewayd 数据目录删除。
      </p>
      <template #footer>
        <n-button size="small" @click="removing = null">取消</n-button>
        <n-button size="small" type="error" :loading="removeBusy" @click="doRemove">
          移除
        </n-button>
      </template>
    </ModalShell>
  </div>
</template>
