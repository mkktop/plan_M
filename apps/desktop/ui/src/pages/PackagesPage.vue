<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { NButton, NInput } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import EmptyState from '@/components/EmptyState.vue'
import ModalShell from '@/components/ModalShell.vue'
import PageHeader from '@/components/PageHeader.vue'
import { isTauriRuntime } from '@/api/ws'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { errorMessage, useToastStore } from '@/stores/toast'
import type { PackageDto, PointDto } from '@/api/types'

const router = useRouter()
const project = useProjectStore()
const conn = useConnectionStore()
const toast = useToastStore()

const selectedId = ref('')
const importOpen = ref(false)
const importPath = ref('')
const importing = ref(false)
const importError = ref('')
const dragOver = ref(false)

const selected = computed<PackageDto | null>(() => {
  if (!project.packages.length) return null
  return project.packages.find((p) => p.id === selectedId.value) ?? project.packages[0]
})

const selectedPoints = computed(() => selected.value?.points ?? [])
const bitCount = computed(() =>
  selectedPoints.value.reduce((n, p) => n + p.bits.length, 0),
)
const rwCount = computed(
  () => selectedPoints.value.filter((p) => p.access === 'rw').length,
)
const selectedUsage = computed(() =>
  selected.value ? project.devices.filter((d) => d.package === selected.value?.id).length : 0,
)

function usageCount(pkgId: string): number {
  return project.devices.filter((d) => d.package === pkgId).length
}

function pointTotal(pkg: PackageDto): number {
  return pkg.points.reduce((n, p) => n + 1 + p.bits.length, 0)
}

function pointMeta(p: PointDto): string {
  const parts: string[] = []
  if (p.scale != null && p.scale !== 1) parts.push(`×${p.scale}`)
  if (p.offset != null && p.offset !== 0) parts.push(`+${p.offset}`)
  return parts.join(' ')
}

function bitsText(p: PointDto): string {
  if (!p.bits.length) return '—'
  return p.bits.map((b) => `b${b.bit} ${b.name}`).join(' / ')
}

function typeText(p: PointDto): string {
  if (p.area === 'coil' || p.area === 'discrete') return 'BOOL'
  return p.type.toUpperCase()
}

function gotoAddDevice(): void {
  if (!selected.value) return
  router.push({
    path: '/devices',
    query: { add: '1', package: selected.value.id },
  })
}

function openImport(): void {
  importError.value = ''
  importOpen.value = true
}

async function doImport(): Promise<void> {
  const path = importPath.value.trim()
  if (!path) {
    importError.value = '请填写设备包目录或 zip 的绝对路径'
    return
  }
  importing.value = true
  importError.value = ''
  const before = new Map(project.packages.map((p) => [p.id, p.version]))
  try {
    await project.importPackage(path)
    const changed =
      project.packages.find((p) => !before.has(p.id)) ??
      project.packages.find((p) => before.get(p.id) !== p.version) ??
      project.packages.find((p) => p.id === selectedId.value)
    if (changed) selectedId.value = changed.id
    toast.success(changed ? `设备包 ${changed.name} v${changed.version} 已安装` : '设备包已导入')
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
      title: directory ? '选择设备包目录' : '选择设备包 zip',
      filters: directory ? undefined : [{ name: '设备包', extensions: ['zip'] }],
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
</script>

<template>
  <div class="page">
    <PageHeader title="设备包" subtitle="声明式设备型号：manifest + 点表；导入即注册，实例共用一份点表">
      <n-button size="small" @click="openImport">
        <template #icon><AppIcon name="download" :size="15" /></template>
        导入设备包
      </n-button>
      <n-button size="small" type="primary" :disabled="!selected" @click="gotoAddDevice">
        <template #icon><AppIcon name="plus" :size="15" /></template>
        用此包添加设备
      </n-button>
    </PageHeader>

    <div v-if="!conn.connected" class="banner warn">
      <AppIcon name="alert" :size="16" />
      <span>未连接 gatewayd，无法导入或查看设备包。</span>
    </div>

    <div v-if="!project.packages.length && project.loaded" class="banner info">
      <AppIcon name="package" :size="16" />
      <span>
        还没有已安装的设备包。导入 <code>manifest.json + points.json</code> 的目录或 zip
        即可支持新型号。
      </span>
      <n-button size="tiny" quaternary @click="openImport">去导入</n-button>
    </div>

    <div class="pkg-layout">
      <aside v-if="project.packages.length" class="card pkg-list">
        <div class="pkg-list-head">
          <span>已安装</span>
          <span class="mono">{{ project.packages.length }}</span>
        </div>
        <button
          v-for="p in project.packages"
          :key="p.id"
          type="button"
          class="pkg-item"
          :class="{ active: selected?.id === p.id }"
          @click="selectedId = p.id"
        >
          <span class="pkg-item-icon"><AppIcon name="package" :size="17" /></span>
          <span class="pkg-item-main">
            <strong>{{ p.name }}</strong>
            <code>{{ p.id }}</code>
            <span class="pkg-item-meta">
              <span class="mono">v{{ p.version }}</span>
              <span>{{ p.vendor || '未署名' }}</span>
              <span>{{ pointTotal(p) }} 点</span>
            </span>
          </span>
          <span class="pkg-usage" :class="{ used: usageCount(p.id) > 0 }" :title="`${usageCount(p.id)} 台设备在用`">
            ×{{ usageCount(p.id) }}
          </span>
        </button>
      </aside>

      <section v-if="selected" class="card pkg-detail">
        <header class="pkg-detail-head">
          <div class="pkg-detail-title">
            <div class="pkg-glyph"><AppIcon name="package" :size="22" /></div>
            <div>
              <h3>{{ selected.name }}</h3>
              <code>{{ selected.id }}</code>
            </div>
          </div>
          <div class="pkg-version">
            <span class="mono">v{{ selected.version }}</span>
            <span class="state-pill idle">{{ selected.driver }}</span>
          </div>
        </header>

        <p v-if="selected.description" class="pkg-desc">{{ selected.description }}</p>

        <div class="pkg-stats">
          <div class="pkg-stat">
            <span class="label">点位</span>
            <strong class="mono">{{ selectedPoints.length }}</strong>
            <span class="muted small">+ {{ bitCount }} 位</span>
          </div>
          <div class="pkg-stat">
            <span class="label">可写点</span>
            <strong class="mono">{{ rwCount }}</strong>
            <span class="muted small">access = rw</span>
          </div>
          <div class="pkg-stat">
            <span class="label">在用设备</span>
            <strong class="mono">{{ selectedUsage }}</strong>
            <span class="muted small">台</span>
          </div>
          <div class="pkg-stat">
            <span class="label">厂商</span>
            <strong>{{ selected.vendor || '—' }}</strong>
            <span class="muted small">vendor</span>
          </div>
        </div>

        <div class="pkg-points-head">
          <h4>点表</h4>
          <span class="muted small">地址为设备手册工程记法；位展开自动生成只读子 tag</span>
        </div>

        <div class="data-table pkg-points">
          <div class="dt-head">
            <span>点位</span>
            <span>地址</span>
            <span>类型</span>
            <span>单位 / 换算</span>
            <span>读写</span>
            <span>位展开</span>
          </div>
          <div v-for="p in selectedPoints" :key="p.id" class="dt-row">
            <div class="cell point-cell">
              <span class="point-name">{{ p.name }}</span>
              <span class="point-id">{{ p.id }}</span>
            </div>
            <code class="cell mono">{{ p.register }}</code>
            <span class="cell"><span class="type-chip">{{ typeText(p) }}</span></span>
            <span class="cell">
              <span v-if="p.unit">{{ p.unit }}</span>
              <span v-else class="muted">—</span>
              <span v-if="pointMeta(p)" class="muted small pkg-scale">{{ pointMeta(p) }}</span>
            </span>
            <span class="cell">
              <span class="state-pill" :class="p.access === 'rw' ? 'warn' : 'idle'">
                {{ p.access === 'rw' ? '读写' : '只读' }}
              </span>
            </span>
            <span class="cell muted small pkg-bits" :title="bitsText(p)">{{ bitsText(p) }}</span>
          </div>
          <div v-if="!selectedPoints.length" class="pkg-empty muted">该包没有点位定义</div>
        </div>
      </section>

      <EmptyState
        v-else-if="project.loaded"
        icon="package"
        title="选择或导入一个设备包"
        text="设备包描述一种设备型号的寄存器点表，导入后可在设备页添加多个同型号从站。"
      >
        <n-button size="small" type="primary" @click="openImport">导入设备包</n-button>
      </EmptyState>
    </div>

    <!-- 导入设备包 -->
    <ModalShell
      v-if="importOpen"
      title="导入设备包"
      subtitle="zip 或已解包的目录；gatewayd 负责校验与安装"
      :width="620"
      @close="importOpen = false"
    >
      <div
        class="drop-zone"
        :class="{ over: dragOver }"
        @dragenter.prevent="dragOver = true"
        @dragover.prevent="dragOver = true"
        @dragleave.prevent="dragOver = false"
        @drop.prevent="onHtmlDrop"
      >
        <div class="drop-icon"><AppIcon name="download" :size="26" /></div>
        <strong>把设备包拖到这里</strong>
        <p class="muted small">
          桌面版支持 zip 文件或已解包的目录（内含 manifest.json + points.json）
        </p>
        <div class="drop-actions">
          <n-button size="small" @click="pickSource(false)">
            <template #icon><AppIcon name="package" :size="14" /></template>
            选择 zip
          </n-button>
          <n-button size="small" @click="pickSource(true)">
            <template #icon><AppIcon name="folder" :size="14" /></template>
            选择目录
          </n-button>
        </div>
        <p v-if="!isTauriRuntime()" class="hint mb-0">
          浏览器调试模式拿不到本地绝对路径，请使用下方路径输入。
        </p>
      </div>

      <div class="field mt-14">
        <label>或填写 gatewayd 本机路径</label>
        <n-input v-model:value="importPath" placeholder="zip 或目录的绝对路径" @keyup.enter="doImport" />
        <p class="hint">
          路径相对 gatewayd 所在机器解析；校验失败会在这里列出具体字段原因。
        </p>
      </div>

      <div v-if="importError" class="import-error">
        <div class="import-error-head">
          <AppIcon name="alert" :size="15" />
          <strong>导入失败</strong>
        </div>
        <pre>{{ importError }}</pre>
      </div>

      <template #footer>
        <n-button size="small" @click="importOpen = false">取消</n-button>
        <n-button size="small" type="primary" :loading="importing" @click="doImport">导入</n-button>
      </template>
    </ModalShell>
  </div>
</template>
