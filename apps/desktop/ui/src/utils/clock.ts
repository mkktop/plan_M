import { ref } from 'vue'

/** 全局秒级时钟：页面上的相对时间/运行时长靠它驱动刷新。 */
export const nowMs = ref(Date.now())

if (typeof window !== 'undefined') {
  window.setInterval(() => {
    nowMs.value = Date.now()
  }, 1000)
}
