import { createRouter, createWebHashHistory } from 'vue-router'

export const router = createRouter({
  // 桌面壳加载本地文件，hash 路由最稳（无需服务端 fallback）。
  history: createWebHashHistory(),
  routes: [
    { path: '/', redirect: '/diagnostics' },
    {
      path: '/diagnostics',
      component: () => import('@/pages/DiagnosticsPage.vue'),
      meta: { title: '诊断' },
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
      path: '/tags',
      component: () => import('@/pages/TagsPage.vue'),
      meta: { title: '实时点表' },
    },
    { path: '/:pathMatch(.*)*', redirect: '/diagnostics' },
  ],
})

export default router
