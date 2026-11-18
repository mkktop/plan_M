import type { DevicePageDto } from '@/api/types'

export const SYSTEM_WORKSPACE = 'system'

export const systemModules = [
  { id: 'home', title: '运行总览', icon: 'home', detail: '采集状态与运行概况', load: () => import('@/pages/HomePage.vue') },
  { id: 'devices', title: '设备', icon: 'chip', detail: '通道与设备管理', load: () => import('@/pages/DevicesPage.vue') },
  { id: 'diagnostics', title: '诊断', icon: 'pulse', detail: '连接、数据浏览与通信日志', load: () => import('@/pages/DiagnosticsPage.vue') },
  { id: 'packages', title: '设备包', icon: 'package', detail: '设备包管理与编辑器', load: () => import('@/pages/PackagesPage.vue') },
  { id: 'plugins', title: '插件', icon: 'plugin', detail: '扩展管理', load: () => import('@/pages/PluginsPage.vue') },
  { id: 'settings', title: '设置', icon: 'settings', detail: '连接、主题与更新', load: () => import('@/pages/SettingsPage.vue') },
]

export const defaultSystemPages: DevicePageDto[] = systemModules.map((module) => ({
  id: module.id,
  title: module.title,
  icon: module.icon,
  components: [{ kind: `system:${module.id}`, title: '', config: null }],
}))

export function systemPagePath(id: string): string {
  return systemModules.some((module) => module.id === id) ? `/${id}` : `/workspace/${id}`
}

export function instanceWorkspaceKey(id: string): string {
  return `device:${id}`
}

export function pluginPageKind(pluginId: string, pageId: string): string {
  return `system-plugin:${pluginId}:${pageId}`
}
