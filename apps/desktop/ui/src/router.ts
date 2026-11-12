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
      // 全局仪表盘已撤出导航（跨设备组态后续再做）；组件仍在设备工作区里使用
      path: '/dashboard',
      redirect: '/home',
    },
    {
      // 通道已并入设备页（按总线分组），保留旧路径避免历史链接失效
      path: '/channels',
      redirect: '/devices',
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
      // 全局实时点表并入诊断页的「数据浏览」标签；保留旧路径与 ?device= 深链
      path: '/tags',
      redirect: (to) => ({
        path: '/diagnostics',
        query: {
          tab: 'data',
          ...(typeof to.query.device === 'string' ? { device: to.query.device } : {}),
        },
      }),
    },
    {
      path: '/settings',
      component: () => import('@/pages/SettingsPage.vue'),
      meta: { title: '设置' },
    },
    { path: '/:pathMatch(.*)*', redirect: '/home' },
  ],
})

export default router
