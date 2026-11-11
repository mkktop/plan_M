//! gatewayd WebSocket 客户端（单例）。
//!
//! 职责：
//! - 握手（hello/token）、断线指数退避重连；
//! - 请求/响应配对（服务端无请求 id，按发送顺序 + 期望响应类型匹配）；
//! - 订阅引用计数去重：同一 tag 被多个组件订阅，只向 gatewayd 发一次；
//! - 重连成功后自动重放订阅集。
//!
//! 不做业务判断；消息由 bridge.ts 分发到 Pinia。

import { reactive } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { ClientMsg, ServerMsg, TagChange } from './types'

export type ConnStatus = 'idle' | 'connecting' | 'connected' | 'reconnecting' | 'error'

export interface GatewaySettings {
  wsUrl: string
  token: string
}

/** Tauri command `gateway_info` 的返回（读 <data-dir>/gateway.token）。 */
export interface GatewayInfo {
  wsUrl: string
  token: string | null
  tokenPath: string
  dataDir: string
  port: number
  hostVersion: string
}

export interface LogEntry {
  id: number
  ts: number
  dir: 'in' | 'out' | 'sys'
  text: string
}

const SETTINGS_KEY = 'hmi.gateway.settings'
const DEFAULT_WS = 'ws://127.0.0.1:59120/ws'

export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export function loadSettings(): GatewaySettings {
  const q = new URLSearchParams(typeof location === 'undefined' ? '' : location.search)
  let saved: Partial<GatewaySettings> = {}
  try {
    saved = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}') as Partial<GatewaySettings>
  } catch {
    saved = {}
  }
  return {
    wsUrl: q.get('ws') || saved.wsUrl || DEFAULT_WS,
    token: q.get('token') || saved.token || '',
  }
}

export function saveSettings(s: GatewaySettings): void {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(s))
  } catch {
    /* 隐私模式下忽略 */
  }
}

/** 桌面壳里从 gateway_info 取地址与 token；浏览器调试模式退回 localStorage / URL query。 */
export async function resolveGatewaySettings(): Promise<{
  settings: GatewaySettings
  info: GatewayInfo | null
}> {
  const fallback = loadSettings()
  const q = new URLSearchParams(location.search)
  if (isTauriRuntime()) {
    try {
      const info = await invoke<GatewayInfo>('gateway_info')
      return {
        settings: {
          wsUrl: q.get('ws') || info.wsUrl || fallback.wsUrl,
          token: q.get('token') || info.token || fallback.token,
        },
        info,
      }
    } catch (e) {
      console.warn('gateway_info 调用失败，使用本地设置', e)
    }
  }
  return { settings: fallback, info: null }
}

interface Pending {
  expect: string[]
  resolve: (m: ServerMsg) => void
  reject: (e: Error) => void
  timer: number
}

class GatewayClient {
  readonly state = reactive({
    status: 'idle' as ConnStatus,
    url: '',
    hostVersion: '',
    lastError: '',
    connectedAt: 0,
    attempts: 0,
    logs: [] as LogEntry[],
    settings: loadSettings(),
    info: null as GatewayInfo | null,
  })

  private ws: WebSocket | null = null
  private pending: Pending[] = []
  private listeners = new Set<(msg: ServerMsg) => void>()
  private statusListeners = new Set<(s: ConnStatus) => void>()
  /** 期望订阅集：tag → 引用计数 */
  private refcount = new Map<string, number>()
  private desired = false
  private authFailed = false
  private reconnectTimer = 0
  private logSeq = 0

  onMessage(cb: (msg: ServerMsg) => void): () => void {
    this.listeners.add(cb)
    return () => this.listeners.delete(cb)
  }

  onStatus(cb: (s: ConnStatus) => void): () => void {
    this.statusListeners.add(cb)
    return () => this.statusListeners.delete(cb)
  }

  /** 应用连接设置并建立连接（重复调用会先断开旧连接）。 */
  async start(settings?: GatewaySettings): Promise<void> {
    if (settings) {
      this.state.settings = { ...settings }
      saveSettings(this.state.settings)
    }
    this.desired = true
    this.state.attempts = 0
    this.closeSocket()
    this.open()
  }

  /** 手动重连（保设置）。 */
  async reconnect(): Promise<void> {
    this.state.attempts = 0
    this.state.lastError = ''
    await this.start()
  }

  disconnect(): void {
    this.desired = false
    this.authFailed = false
    this.clearReconnect()
    this.rejectAll(new Error('已断开连接'))
    this.closeSocket()
    this.setState('idle')
  }

  /** 命令请求；按发送顺序匹配期望的响应类型。 */
  request<T extends ServerMsg = ServerMsg>(
    msg: ClientMsg,
    expect: string | string[],
    timeoutMs = 10_000,
  ): Promise<T> {
    const expects = Array.isArray(expect) ? expect : [expect]
    return new Promise<T>((resolve, reject) => {
      if (!this.sendRaw(msg)) {
        reject(new Error('未连接到 gatewayd'))
        return
      }
      const entry: Pending = {
        expect: expects,
        resolve: (m) => resolve(m as T),
        reject,
        timer: 0,
      }
      entry.timer = window.setTimeout(() => {
        const i = this.pending.indexOf(entry)
        if (i >= 0) this.pending.splice(i, 1)
        reject(new Error('请求超时：gatewayd 未响应'))
      }, timeoutMs)
      this.pending.push(entry)
    })
  }

  /** 订阅 tag（引用计数）。返回退订函数。 */
  subscribe(tags: string[]): () => void {
    const fresh: string[] = []
    for (const t of tags) {
      const n = this.refcount.get(t) ?? 0
      this.refcount.set(t, n + 1)
      if (n === 0) fresh.push(t)
    }
    if (fresh.length > 0 && this.state.status === 'connected') {
      this.sendRaw({ type: 'subscribe', tags: fresh })
    }
    let active = true
    return () => {
      if (!active) return
      active = false
      const gone: string[] = []
      for (const t of tags) {
        const n = (this.refcount.get(t) ?? 1) - 1
        if (n <= 0) {
          this.refcount.delete(t)
          gone.push(t)
        } else {
          this.refcount.set(t, n)
        }
      }
      if (gone.length > 0 && this.state.status === 'connected') {
        this.sendRaw({ type: 'unsubscribe', tags: gone })
      }
    }
  }

  clearLogs(): void {
    this.state.logs.splice(0)
  }

  // —— 内部 ——

  private open(): void {
    this.clearReconnect()
    const url = this.state.settings.wsUrl
    if (!url) {
      this.state.lastError = '未配置 gatewayd 地址'
      this.setState('error')
      return
    }
    this.state.url = url
    this.authFailed = false
    this.setState(this.state.attempts > 0 ? 'reconnecting' : 'connecting')

    let ws: WebSocket
    try {
      ws = new WebSocket(url)
    } catch (e) {
      this.state.lastError = e instanceof Error ? e.message : String(e)
      this.scheduleReconnect('地址无效')
      return
    }
    this.ws = ws

    const authTimer = window.setTimeout(() => {
      if (this.ws === ws && this.state.status !== 'connected') {
        this.log('sys', '握手超时（8s），关闭连接重试')
        ws.close()
      }
    }, 8000)

    ws.onopen = () => {
      this.log('sys', `socket 已打开 → ${url}`)
      this.sendRaw({ type: 'hello', token: this.state.settings.token })
    }

    ws.onmessage = (ev) => {
      if (typeof ev.data !== 'string') return
      let msg: ServerMsg
      try {
        msg = JSON.parse(ev.data) as ServerMsg
      } catch {
        this.log('sys', '收到无法解析的消息')
        return
      }
      this.log('in', describeServer(msg))

      if (msg.type === 'welcome') {
        window.clearTimeout(authTimer)
        this.state.hostVersion = msg.hostVersion
        this.state.connectedAt = Date.now()
        this.state.attempts = 0
        this.state.lastError = ''
        this.setState('connected')
        this.resubscribeAll()
      }
      if (msg.type === 'error' && this.state.status !== 'connected') {
        // 握手阶段的 error 基本是 token 不匹配；服务端随后断开，不再自动重试。
        this.state.lastError = msg.message
        this.authFailed = true
      }

      this.resolvePending(msg)
      for (const cb of this.listeners) cb(msg)
    }

    ws.onclose = (ev) => {
      window.clearTimeout(authTimer)
      if (this.ws !== ws) return
      this.ws = null
      this.state.connectedAt = 0
      this.rejectAll(new Error('与 gatewayd 的连接已断开'))
      if (!this.desired) {
        this.setState('idle')
        return
      }
      if (this.authFailed) {
        this.authFailed = false
        this.desired = false
        this.state.lastError = this.state.lastError || '鉴权失败'
        this.setState('error')
        return
      }
      this.scheduleReconnect(ev.reason || '连接关闭')
    }

    ws.onerror = () => {
      // 具体原因在 onclose 统一处理
    }
  }

  private closeSocket(): void {
    const ws = this.ws
    this.ws = null
    if (ws) {
      try {
        ws.close()
      } catch {
        /* ignore */
      }
    }
  }

  private scheduleReconnect(reason: string): void {
    if (!this.desired) return
    this.state.attempts += 1
    this.state.lastError = reason
    this.setState('reconnecting')
    const delay = Math.min(500 * Math.pow(1.7, this.state.attempts - 1), 5000)
    const jitter = Math.random() * 250
    this.reconnectTimer = window.setTimeout(() => {
      if (this.desired) this.open()
    }, delay + jitter)
  }

  private clearReconnect(): void {
    if (this.reconnectTimer) {
      window.clearTimeout(this.reconnectTimer)
      this.reconnectTimer = 0
    }
  }

  private setState(s: ConnStatus): void {
    if (this.state.status === s) return
    this.state.status = s
    for (const cb of this.statusListeners) cb(s)
  }

  private sendRaw(msg: ClientMsg): boolean {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return false
    try {
      this.ws.send(JSON.stringify(msg))
    } catch {
      return false
    }
    this.log('out', describeClient(msg))
    return true
  }

  private resubscribeAll(): void {
    const tags = [...this.refcount.entries()].filter(([, n]) => n > 0).map(([t]) => t)
    if (tags.length === 0) return
    this.sendRaw({ type: 'subscribe', tags })
  }

  private resolvePending(msg: ServerMsg): void {
    const i = this.pending.findIndex((p) => p.expect.includes(msg.type))
    if (i < 0) return
    const [p] = this.pending.splice(i, 1)
    window.clearTimeout(p.timer)
    if (msg.type === 'error') p.reject(new Error(msg.message))
    else p.resolve(msg)
  }

  private rejectAll(err: Error): void {
    const list = this.pending.splice(0)
    for (const p of list) {
      window.clearTimeout(p.timer)
      p.reject(err)
    }
  }

  private log(dir: LogEntry['dir'], text: string): void {
    this.state.logs.unshift({ id: ++this.logSeq, ts: Date.now(), dir, text })
    if (this.state.logs.length > 300) this.state.logs.length = 300
  }
}

function describeServer(m: ServerMsg): string {
  switch (m.type) {
    case 'welcome':
      return `welcome · host ${m.hostVersion}`
    case 'snapshot':
      return `snapshot · 通道 ${m.channels.length} · 设备 ${m.devices.length} · 包 ${m.packages.length} · tag ${m.tags.length}`
    case 'changes':
      return `changes ×${m.items.length}${m.items[0] ? ` · ${m.items[0].tag}…` : ''}`
    case 'channelState':
      return `channelState · ${m.channel} → ${m.state}${m.detail ? ` (${m.detail})` : ''}`
    case 'deviceState':
      return `deviceState · ${m.device} → ${m.online ? 'online' : 'offline'}`
    case 'scanProgress':
      return `scanProgress · ${m.channel} #${m.slave} ${m.found ? 'found' : '-'}`
    case 'scanDone':
      return `scanDone · ${m.channel} · 发现 ${m.found.length} 个从站`
    case 'serialPorts':
      return `serialPorts · ${m.ports.length} 个串口`
    case 'packages':
      return `packages · ${m.packages.length} 个设备包`
    case 'dashboards':
      return `dashboards · ${m.dashboards.length} 个布局`
    case 'uiPlugins':
      return `uiPlugins · ${m.plugins.length} 个插件`
    case 'commandOk':
      return `commandOk · ${m.kind}`
    case 'error':
      return `error · ${m.message}`
    default:
      return 'unknown'
  }
}

function describeClient(m: ClientMsg): string {
  switch (m.type) {
    case 'hello':
      return 'hello · token ******'
    case 'subscribe':
      return `subscribe ×${m.tags.length}`
    case 'unsubscribe':
      return `unsubscribe ×${m.tags.length}`
    case 'writeTag':
      return `writeTag · ${m.tag} = ${JSON.stringify(m.value)}`
    case 'scanChannel':
      return `scanChannel · ${m.channel} ${m.range ? `${m.range[0]}-${m.range[1]}` : ''}`
    case 'upsertChannel':
      return `upsertChannel · ${m.config.id} (${m.config.port})`
    case 'removeChannel':
      return `removeChannel · ${m.channel}`
    case 'addDevice':
      return `addDevice · ${m.config.id} @ ${m.config.channelId}#${m.config.slave}`
    case 'removeDevice':
      return `removeDevice · ${m.device}`
    case 'importPackage':
      return `importPackage · ${m.path}`
    case 'getSnapshot':
      return 'getSnapshot'
    case 'getSerialPorts':
      return 'getSerialPorts'
    case 'getPackages':
      return 'getPackages'
    case 'getDashboards':
      return 'getDashboards'
    case 'saveDashboard':
      return `saveDashboard · ${m.dashboard.id} (${m.dashboard.widgets.length} 组件)`
    case 'removeDashboard':
      return `removeDashboard · ${m.id}`
    case 'getUiPlugins':
      return 'getUiPlugins'
    case 'importUiPlugin':
      return `importUiPlugin · ${m.path}`
    case 'removeUiPlugin':
      return `removeUiPlugin · ${m.id}`
    default:
      return 'unknown'
  }
}

export const gateway = new GatewayClient()

export type { TagChange }
