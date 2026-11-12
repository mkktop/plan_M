<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { darkTheme, dateZhCN, zhCN, type GlobalThemeOverrides } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import ConnectionPill from '@/components/ConnectionPill.vue'
import DeviceTabs from '@/components/DeviceTabs.vue'
import ToastStack from '@/components/ToastStack.vue'
import WindowControls from '@/components/WindowControls.vue'
import { gateway, isTauriRuntime, resolveGatewaySettings } from '@/api/ws'
import { installGatewayBridge } from '@/api/bridge'
import { pluginHost } from '@/plugins/host'
import { useDeviceTabsStore } from '@/stores/deviceTabs'
import { useProjectStore } from '@/stores/project'
import { useWorkspacesStore } from '@/stores/workspaces'
import { pagesForPackage } from '@/utils/devicePages'
import { nowMs } from '@/utils/clock'
import { addressLabel } from '@/utils/transport'

const route = useRoute()
const router = useRouter()
const project = useProjectStore()
const workspaces = useWorkspacesStore()
const deviceTabs = useDeviceTabsStore()

interface NavItem {
  path: string
  label: string
  desc: string
  icon: string
  query?: Record<string, string>
}

const baseNav: NavItem[] = [
  { path: '/home', label: '首页', desc: '欢迎与总览', icon: 'home' },
  { path: '/diagnostics', label: '诊断', desc: '连接 / 日志 / 数据', icon: 'pulse' },
  { path: '/devices', label: '设备', desc: '总线与从站', icon: 'chip' },
  { path: '/packages', label: '设备包', desc: '型号与点表', icon: 'package' },
  { path: '/plugins', label: '插件', desc: 'ESM UI 扩展', icon: 'plugin' },
  { path: '/settings', label: '设置', desc: '连接与更新', icon: 'settings' },
]

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
    ? pagesForPackage(activePackage.value, workspaces.get(activePackage.value.id))
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

// 设备模式：左侧是该设备的页面；系统模式：全局页面 + 插件导航页。
const nav = computed<NavItem[]>(() => {
  const device = activeDevice.value
  if (device) {
    return devicePages.value.map((page) => ({
      path: `/d/${device.id}/${page.id}`,
      label: page.title,
      desc: '设备页面',
      icon: page.icon || 'grid',
      query: undefined,
    }))
  }
  return [
    ...baseNav,
    ...pluginHost.pages.map((p) => ({
      path: p.path,
      label: p.title,
      desc: p.pluginName,
      icon: p.icon,
      query: undefined,
    })),
  ]
})

const themeOverrides: GlobalThemeOverrides = {
  common: {
    primaryColor: '#4db5ff',
    primaryColorHover: '#6cc3ff',
    primaryColorPressed: '#2f9df0',
    primaryColorSuppl: '#4db5ff',
    infoColor: '#4db5ff',
    successColor: '#35d399',
    warningColor: '#f5b942',
    errorColor: '#ff6b6b',
    bodyColor: '#0a0d12',
    cardColor: '#0f141b',
    modalColor: '#10161f',
    popoverColor: '#10161f',
    tableColor: '#0f141b',
    inputColor: '#0b1016',
    borderColor: '#26313f',
    dividerColor: '#1c2531',
    textColorBase: '#e8eef7',
    textColor1: '#e8eef7',
    textColor2: '#c2cedd',
    textColor3: '#8494a8',
    textColorDisabled: '#5a6879',
    borderRadius: '6px',
    borderRadiusSmall: '4px',
    fontFamily: 'Inter, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif',
    fontFamilyMono: 'ui-monospace, "Cascadia Mono", Consolas, monospace',
    fontSize: '13px',
    fontSizeSmall: '12px',
    fontSizeMedium: '14px',
  },
}

const title = computed(() => {
  if (inDeviceWorkspace.value && activeDevice.value) {
    const deviceName = activeDevice.value.name || activeDevice.value.id
    return activePage.value ? `${deviceName} · ${activePage.value.title}` : deviceName
  }
  return (route.meta.title as string | undefined) ?? '首页'
})
const kicker = computed(() =>
  inDeviceWorkspace.value && activePackage.value
    ? `设备工作区 · ${activePackage.value.name}`
    : '工业数据采集 · 上位机平台',
)
const clock = computed(() =>
  new Date(nowMs.value).toLocaleTimeString('zh-CN', { hour12: false }),
)
const runtime = computed(() => (isTauriRuntime() ? 'desktop' : 'web-dev'))

onMounted(async () => {
  installGatewayBridge()
  const { settings, info } = await resolveGatewaySettings()
  gateway.state.info = info
  await gateway.start(settings)
})

onBeforeUnmount(() => {
  gateway.disconnect()
})
</script>

<template>
  <n-config-provider :theme="darkTheme" :locale="zhCN" :date-locale="dateZhCN" :theme-overrides="themeOverrides">
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
            <div class="brand-sub">上位机平台</div>
          </div>
        </div>

        <div class="titlebar-context">
          <span class="titlebar-kicker">{{ kicker }}</span>
          <h1>{{ title }}</h1>
        </div>

        <div class="titlebar-tools">
          <span class="clock mono">{{ clock }}</span>
          <ConnectionPill @click="router.push('/diagnostics')" />
          <WindowControls />
        </div>
      </header>

      <aside class="rail">
        <div v-if="inDeviceWorkspace && activeDevice" class="rail-context">
          <span class="rail-context-label">当前设备</span>
          <strong>{{ activeDevice.name || activeDevice.id }}</strong>
          <span class="mono muted small">
            {{ addressLabel(activeDevice.address) }} · {{ activePackage?.name || activeDevice.id }}
          </span>
        </div>

        <nav class="nav">
          <router-link
            v-for="item in nav"
            :key="item.path"
            :to="item.query ? { path: item.path, query: item.query } : item.path"
            class="nav-item"
            active-class="active"
          >
            <AppIcon :name="item.icon" :size="18" />
            <span class="nav-text">
              <span>{{ item.label }}</span>
              <small>{{ item.desc }}</small>
            </span>
          </router-link>
        </nav>

        <div class="rail-system-wrap">
          <button
            type="button"
            class="rail-system"
            :class="{ active: !inDeviceWorkspace }"
            title="切换到系统菜单"
            @click="router.push('/home')"
          >
            <AppIcon name="layout" :size="17" />
            <span class="nav-text">
              <span>切换系统菜单</span>
              <small>通道 / 设备包 / 插件</small>
            </span>
          </button>
        </div>

        <div class="rail-foot">
          <div class="rail-status">
            <span class="online-dot" :class="{ on: gateway.state.status === 'connected' }" />
            <span class="mono">v{{ gateway.state.hostVersion || '0.1.0' }}</span>
          </div>
          <span class="muted small">{{ runtime }}</span>
        </div>
      </aside>

      <div class="main">
        <DeviceTabs />

        <main class="content">
          <router-view />
        </main>
      </div>
    </div>
    <ToastStack />
  </n-config-provider>
</template>
