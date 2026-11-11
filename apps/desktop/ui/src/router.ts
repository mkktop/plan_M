import { createRouter, createWebHashHistory } from 'vue-router'

export const router = createRouter({
  // 桌面壳加载本地文件，hash 路由最稳（无需服务端 fallback）。
  history: createWebHashHistory(),
  routes: [
    { path: '/', redirect: '/home' },
    {
      path: '/home',
      component: () => import('@/pages/HomePage.vue'),
      meta: { title: '首页' },
    },
    {
      path: '/diagnostics',
      component: () => import('@/pages/DiagnosticsPage.vue'),
      meta: { title: '诊断' },
    },
    {
      path: '/dashboard',
      component: () => import('@/pages/DashboardPage.vue'),
      meta: { title: '仪表盘' },
    },
    {
      path: '/channels',
      component: () => import('@/pages/ChannelsPage.vue'),
      meta: { title: '通道' },
    },
    {
      path: '/devices',
      component: () => import('@/pages/DevicesPage.vue'),
      meta: { title: '设备' },
    },
    {
      path: '/packages',
      component: () => import('@/pages/PackagesPage.vue'),
      meta: { title: '设备包' },
    },
    {
      path: '/plugins',
      component: () => import('@/pages/PluginsPage.vue'),
      meta: { title: '插件' },
    },
    {
      path: '/plugin/:pluginId/:pageId',
      component: () => import('@/pages/PluginPage.vue'),
      meta: { title: '插件页面' },
    },
    {
      path: '/t/:typeId/:pageId?',
      component: () => import('@/pages/DeviceWorkspacePage.vue'),
      meta: { title: '设备工作区' },
    },
    {
      path: '/tags',
      component: () => import('@/pages/TagsPage.vue'),
      meta: { title: '实时点表' },
    },
    { path: '/:pathMatch(.*)*', redirect: '/home' },
  ],
})

export default router
