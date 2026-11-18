<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { dateZhCN, NConfigProvider, zhCN } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import CommandPalette from '@/components/CommandPalette.vue'
import ConnectionPill from '@/components/ConnectionPill.vue'
import DeviceTabs from '@/components/DeviceTabs.vue'
import DevicePickerModal from '@/components/device/DevicePickerModal.vue'
import WorkspaceCanvas from '@/components/device/WorkspaceCanvas.vue'
import DevicePagesEditor from '@/components/DevicePagesEditor.vue'
import EmptyState from '@/components/EmptyState.vue'
import ToastStack from '@/components/ToastStack.vue'
import WindowControls from '@/components/WindowControls.vue'
import { gateway, isTauriRuntime, resolveGatewaySettings } from '@/api/ws'
import { installGatewayBridge } from '@/api/bridge'
import { pluginHost } from '@/plugins/host'
import { useDeviceTabsStore } from '@/stores/deviceTabs'
import { useConnectionStore } from '@/stores/connection'
import { useProjectStore } from '@/stores/project'
import { useWorkspacesStore } from '@/stores/workspaces'
import { pagesForPackage } from '@/utils/devicePages'
import { defaultSystemPages, instanceWorkspaceKey, pluginPageKind, systemModules, systemPagePath, SYSTEM_WORKSPACE } from '@/utils/systemPages'
import type { DevicePageDto } from '@/api/types'
import { addressLabel } from '@/utils/transport'
// 主题：状态与 Naive 覆盖全部来自 utils/theme.ts + tokens.css（单一真源）。
import { cycleThemeMode, isDark, naiveTheme, themeMode, themeOverrides } from '@/utils/theme'

const route = useRoute()
const router = useRouter()
const project = useProjectStore()
const workspaces = useWorkspacesStore()
const deviceTabs = useDeviceTabsStore()
const connection = useConnectionStore()

interface NavItem {
  path: string
  label: string
  desc: string
  icon: string
}

const protectedSystemPages = systemModules.map(({ id }) => id)
const managementPages = new Set(['packages', 'plugins', 'settings'])
const systemLayout = computed(() => workspaces.get(SYSTEM_WORKSPACE))
const systemPages = computed(() => {
  const saved = systemLayout.value?.pages ?? []
  return [...saved, ...defaultSystemPages.filter((page) => !saved.some(({ id }) => id === page.id))]
})
const baseNav = computed<NavItem[]>(() => systemPages.value.map((page) => ({
  path: systemPagePath(page.id), label: page.title, icon: page.icon || 'grid',
  desc: systemModules.find(({ id }) => id === page.id)?.detail ?? '自定义工作台页面',
})))
const pluginRoute = computed(() => typeof route.params.pluginId === 'string' && typeof route.params.pageId === 'string'
  ? { pluginId: route.params.pluginId, pageId: route.params.pageId } : null)
const customSystemRoute = computed(() => route.path.startsWith('/workspace/'))
const systemRouteId = computed(() => customSystemRoute.value ? String(route.params.pageId ?? '') :
  systemModules.find(({ id }) => route.path === `/${id}`)?.id)
const workspaceKey = computed(() => pluginRoute.value
  ? pluginPageKind(pluginRoute.value.pluginId, pluginRoute.value.pageId)
  : systemRouteId.value ? SYSTEM_WORKSPACE : '')
const systemOverride = computed(() => workspaces.get(workspaceKey.value))
const editablePages = computed<DevicePageDto[]>(() => {
  if (!pluginRoute.value) return systemPages.value
  if (systemOverride.value?.pages.length) return systemOverride.value.pages
  const plugin = pluginHost.pages.find(({ path }) => path === route.path)
  return [{ id: 'page', title: plugin?.title ?? '插件页面', icon: plugin?.icon ?? 'plugin',
    components: [{ kind: workspaceKey.value, title: '', config: null }] }]
})
const systemPage = computed(() => workspaceKey.value
  ? editablePages.value.find(({ id }) => id === (pluginRoute.value ? 'page' : systemRouteId.value)) : undefined)
const systemEditorOpen = ref(false)
watch(workspaceKey, () => { systemEditorOpen.value = false })

const activeDeviceId = computed(() =>
  typeof route.params.deviceId === 'string' ? route.params.deviceId : '',
)
const activeDevice = computed(
  () => project.devices.find((d) => d.id === activeDeviceId.value) ?? null,
)
const activePackage = computed(() => project.packageOf(activeDevice.value))
const inDeviceWorkspace = computed(() => Boolean(activeDeviceId.value && activeDevice.value))
const devicePages = computed(() =>
  activePackage.value
    ? pagesForPackage(activePackage.value, workspaces.get(instanceWorkspaceKey(activeDeviceId.value)) ?? workspaces.get(activePackage.value.id))
    : [],
)
const activePageId = computed(() =>
  typeof route.params.pageId === 'string' ? route.params.pageId : '',
)
const activePage = computed(
  () => devicePages.value.find((p) => p.id === activePageId.value) ?? devicePages.value[0] ?? null,
)

// 当前设备自动打开标签；设备被删除时清掉标签并离开该工作区。
watch(
  activeDeviceId,
  (id) => {
    if (id) deviceTabs.open(id)
  },
  { immediate: true },
)

watch(
  () => project.devices.map((d) => d.id).join(','),
  () => {
    deviceTabs.prune(new Set(project.devices.map((d) => d.id)))
    const current = activeDeviceId.value
    if (current && !project.devices.some((d) => d.id === current)) {
      const next = deviceTabs.openIds[0]
      void router.replace(next ? `/d/${next}` : '/devices')
    }
  },
)

const pluginNav = computed<NavItem[]>(() =>
  pluginHost.pages.map((p) => {
    const custom = workspaces.get(pluginPageKind(p.pluginId, p.pageId))?.pages[0]
    return { path: p.path, label: custom?.title ?? p.title, desc: p.pluginName, icon: custom?.icon ?? p.icon }
  }),
)
const navGroups = computed(() => {
  const device = activeDevice.value
  if (device) {
    const pages = devicePages.value.map((page) => ({
      path: `/d/${device.id}/${page.id}`,
      label: page.title,
      desc: '设备页面',
      icon: page.icon || 'grid',
    }))
    return [{ label: '设备页面', items: pages }]
  }
  return [
    { label: '监控', items: baseNav.value.filter((item) => !managementPages.has(item.path.slice(1))) },
    { label: '管理', items: [...baseNav.value.filter((item) => managementPages.has(item.path.slice(1))), ...pluginNav.value] },
  ]
})
const commands = computed(() =>
  [...baseNav.value, ...pluginNav.value].map((item) => ({ ...item, detail: item.desc })),
)

const title = computed(() => {
  if (inDeviceWorkspace.value && activeDevice.value) {
    const deviceName = activeDevice.value.name || activeDevice.value.id
    return activePage.value ? `${deviceName} · ${activePage.value.title}` : deviceName
  }
  return systemPage.value?.title ?? (route.meta.title as string | undefined) ?? '首页'
})
const kicker = computed(() =>
  inDeviceWorkspace.value && activePackage.value
    ? `设备工作区 · ${activePackage.value.name}`
    : '工作台',
)
const runtime = isTauriRuntime() ? '桌面工作台' : '浏览器工作台'
const commandOpen = ref(false)
const devicePickerOpen = ref(false)

function pickDevice(id: string): void {
  devicePickerOpen.value = false
  void router.push(`/d/${id}`)
}

function onShortcut(event: KeyboardEvent): void {
  if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== 'k') return
  if (event.repeat || event.isComposing) return
  if (!commandOpen.value && document.querySelector('[aria-modal="true"]')) return
  event.preventDefault()
  commandOpen.value = !commandOpen.value
}

// 主题切换按钮：深色 → 浅色 → 跟随系统循环。
const THEME_META = {
  dark: { icon: 'moon', label: '深色' },
  light: { icon: 'sun', label: '浅色' },
  system: { icon: 'monitor', label: '跟随系统' },
} as const
const themeIcon = computed(() => THEME_META[themeMode.value].icon)
const themeTitle = computed(
  () => `主题：${THEME_META[themeMode.value].label}（点击切换）`,
)

onMounted(async () => {
  window.addEventListener('keydown', onShortcut)
  installGatewayBridge()
  const { settings, info } = await resolveGatewaySettings()
  gateway.state.info = info
  await gateway.start(settings)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onShortcut)
  gateway.disconnect()
})
</script>

<template>
  <n-config-provider
    abstract
    :theme="naiveTheme(isDark)"
    :locale="zhCN"
    :date-locale="dateZhCN"
    :theme-overrides="themeOverrides"
  >
    <div class="app-shell">
      <!-- 自绘标题栏：窗口无系统边框，这一条就是窗口的标题栏（拖拽区 + 窗口按钮） -->
      <header class="titlebar" data-tauri-drag-region="deep">
        <div class="titlebar-brand">
          <div class="brand-glyph">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <path d="M3 13h4l2.5-7 4 13 2-6h5.5" />
            </svg>
          </div>
          <div class="brand-text">
            <div class="brand-name">planM</div>
            <div class="brand-sub">采集工作台</div>
          </div>
        </div>

        <div class="titlebar-context">
          <span class="titlebar-kicker">{{ kicker }}</span>
          <AppIcon name="chevronDown" :size="12" />
          <span class="titlebar-title">{{ title }}</span>
        </div>

        <div class="titlebar-tools">
          <button v-if="systemPage && workspaces.loaded && connection.connected" type="button"
            class="titlebar-tool" title="编辑工作台页面" aria-label="编辑工作台页面"
            data-tauri-drag-region="false" @click="systemEditorOpen = true">
            <AppIcon name="edit" :size="16" />
          </button>
          <button
            v-if="!deviceTabs.openIds.length && project.devices.length"
            type="button"
            class="titlebar-tool"
            title="打开设备"
            aria-label="打开设备"
            data-tauri-drag-region="false"
            @click="devicePickerOpen = true"
          >
            <AppIcon name="chip" :size="16" />
          </button>
          <button
            type="button"
            class="titlebar-search"
            aria-label="快速跳转"
            data-tauri-drag-region="false"
            @click="commandOpen = true"
          >
            <AppIcon name="search" :size="16" />
            <span>快速跳转</span>
            <kbd>Ctrl K</kbd>
          </button>
          <button
            type="button"
            class="titlebar-tool"
            :title="themeTitle"
            :aria-label="themeTitle"
            data-tauri-drag-region="false"
            @click="cycleThemeMode()"
          >
            <AppIcon :name="themeIcon" :size="15" />
          </button>
          <ConnectionPill data-tauri-drag-region="false" @click="router.push('/diagnostics')" />
          <WindowControls />
        </div>
      </header>

      <aside class="rail">
        <div v-if="inDeviceWorkspace && activeDevice" class="rail-context">
          <span class="rail-context-label">当前设备</span>
          <div class="rail-context-title">
            <span class="online-dot" :class="{ on: project.live && activeDevice.online }" />
            <strong>{{ activeDevice.name || activeDevice.id }}</strong>
          </div>
          <span class="mono muted small">
            {{ addressLabel(activeDevice.address) }} · {{ project.live ? (activeDevice.online ? '在线' : '离线') : '状态待确认' }}
          </span>
        </div>

        <nav class="nav" aria-label="主导航">
          <div v-for="group in navGroups" :key="group.label" class="nav-group">
            <div class="nav-group-label">{{ group.label }}</div>
            <router-link
              v-for="item in group.items"
              :key="item.path"
              :to="item.path"
              class="nav-item"
              :class="{ active: route.path === item.path || (inDeviceWorkspace && item.path === `/d/${activeDeviceId}/${activePage?.id}`) }"
              :title="item.label"
            >
              <AppIcon :name="item.icon" :size="18" />
              <span class="nav-text">{{ item.label }}</span>
            </router-link>
          </div>
        </nav>

        <div v-if="inDeviceWorkspace" class="rail-system-wrap">
          <button
            type="button"
            class="rail-system"
            title="切换到系统菜单"
            @click="router.push('/home')"
          >
            <AppIcon name="layout" :size="17" />
            <span class="nav-text">返回工作台</span>
          </button>
        </div>

        <div class="rail-foot" :title="runtime">
          <span class="rail-runtime-icon"><AppIcon name="monitor" :size="17" /></span>
          <div class="rail-runtime">
            <strong>{{ runtime }}</strong>
            <span v-if="gateway.state.hostVersion" class="mono">网关 v{{ gateway.state.hostVersion }}</span>
          </div>
        </div>
      </aside>

      <div class="main">
        <DeviceTabs />

        <main class="content">
          <div v-if="systemPage" class="system-workspace" :key="route.path">
            <WorkspaceCanvas :components="systemPage.components" />
          </div>
          <EmptyState v-else-if="customSystemRoute" icon="layout"
            :title="workspaces.loaded ? '页面不存在' : '正在加载工作台'"
            :text="workspaces.loaded ? '该页面可能已被删除或恢复为默认布局。' : '连接采集网关后加载已保存的页面。'">
            <router-link to="/home">返回运行总览</router-link>
          </EmptyState>
          <router-view v-else />
        </main>
      </div>
    </div>
    <DevicePagesEditor v-if="systemEditorOpen && systemPage" system type-id=""
      :workspace-key="workspaceKey"
      :package-name="pluginRoute ? systemPage.title : '工作台'"
      :scope-label="pluginRoute ? '插件页面' : '项目工作台'"
      inherited-label="默认布局"
      :single-page="Boolean(pluginRoute)"
      :protected-page-ids="pluginRoute ? ['page'] : protectedSystemPages"
      :pages="editablePages"
      :initial-page-id="systemPage.id"
      :overridden="Boolean(systemOverride)"
      @close="systemEditorOpen = false"
    />
    <CommandPalette v-if="commandOpen" :pages="commands" @close="commandOpen = false" />
    <DevicePickerModal v-if="devicePickerOpen" @close="devicePickerOpen = false" @pick="pickDevice" />
    <ToastStack />
  </n-config-provider>
</template>
