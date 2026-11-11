<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { darkTheme, dateZhCN, zhCN, type GlobalThemeOverrides } from 'naive-ui'
import AppIcon from '@/components/AppIcon.vue'
import ConnectionPill from '@/components/ConnectionPill.vue'
import ToastStack from '@/components/ToastStack.vue'
import { gateway, isTauriRuntime, resolveGatewaySettings } from '@/api/ws'
import { installGatewayBridge } from '@/api/bridge'
import { nowMs } from '@/utils/clock'

const route = useRoute()
const router = useRouter()

const nav = [
  { path: '/diagnostics', label: '诊断', desc: '连接与健康', icon: 'pulse' },
  { path: '/channels', label: '通道', desc: '串口与总线', icon: 'plug' },
  { path: '/devices', label: '设备', desc: '从站与设备包', icon: 'chip' },
  { path: '/tags', label: '实时点表', desc: '在线数据', icon: 'grid' },
]

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

const title = computed(() => (route.meta.title as string | undefined) ?? '上位机平台')
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
      <aside class="rail">
        <div class="brand">
          <div class="brand-glyph">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
              <path d="M3 13h4l2.5-7 4 13 2-6h5.5" />
            </svg>
          </div>
          <div class="brand-text">
            <div class="brand-name">HMI</div>
            <div class="brand-sub">PLAN M</div>
          </div>
        </div>

        <nav class="nav">
          <router-link
            v-for="item in nav"
            :key="item.path"
            :to="item.path"
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

        <div class="rail-foot">
          <div class="rail-status">
            <span class="online-dot" :class="{ on: gateway.state.status === 'connected' }" />
            <span class="mono">v{{ gateway.state.hostVersion || '0.1.0' }}</span>
          </div>
          <span class="muted small">{{ runtime }}</span>
        </div>
      </aside>

      <div class="main">
        <header class="topbar">
          <div class="crumb">
            <span class="crumb-kicker">工业数据采集 · 上位机平台</span>
            <h1>{{ title }}</h1>
          </div>
          <div class="topbar-right">
            <span class="clock mono">{{ clock }}</span>
            <ConnectionPill @click="router.push('/diagnostics')" />
          </div>
        </header>

        <main class="content">
          <router-view />
        </main>
      </div>
    </div>
    <ToastStack />
  </n-config-provider>
</template>
