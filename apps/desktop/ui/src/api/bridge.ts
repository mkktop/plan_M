//! WS 消息 → Pinia 的唯一分发点。页面只读 store，不自己解析服务端消息。

import { gateway } from './ws'
import { useProjectStore } from '@/stores/project'
import { useTagsStore } from '@/stores/tags'
import { useConnectionStore } from '@/stores/connection'
import { useToastStore } from '@/stores/toast'

let installed = false

export function installGatewayBridge(): void {
  if (installed) return
  installed = true

  const project = useProjectStore()
  const tags = useTagsStore()
  const conn = useConnectionStore()
  const toast = useToastStore()

  gateway.onMessage((msg) => {
    switch (msg.type) {
      case 'welcome':
        conn.state.hostVersion = msg.hostVersion
        // 重连/首连都拉一次全量：UI 可能在断线期间错过了配置变更。
        void project.refresh().catch((e) => {
          console.warn('拉取快照失败', e)
        })
        break
      case 'snapshot':
        project.applySnapshot(msg)
        tags.applyMany(msg.tags)
        break
      case 'changes':
        tags.applyMany(msg.items)
        break
      case 'channelState':
        project.applyChannelState(msg.channel, msg.state)
        break
      case 'deviceState':
        project.applyDeviceState(msg.device, msg.online)
        break
      case 'packages':
        project.applyPackages(msg.packages)
        break
      case 'error':
        // 握手阶段（未连接）的错误没有页面请求可 catch，这里兜底提示。
        if (!conn.connected) toast.error(msg.message)
        break
      default:
        break
    }
  })
}
