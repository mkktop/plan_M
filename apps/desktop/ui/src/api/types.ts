//! gatewayd WS 契约的 TS 镜像（对应 crates/wire/src/lib.rs）。
//! 只做类型声明；任何业务真值都以 gatewayd 为准。

export type Quality = 'good' | 'bad' | 'uncertain' | 'comm_lost'

export type TagValue =
  | { f64: number }
  | { i64: number }
  | { bool: boolean }
  | { str: string }

export interface TagChange {
  tag: string
  value: TagValue
  quality: Quality
  tsMs: number
}

export type ChannelState = 'running' | 'recovering' | 'faulted' | 'stopped'
export type Parity = 'none' | 'odd' | 'even'

export interface ChannelDto {
  id: string
  name: string
  port: string
  state: string
  hwId: string | null
  baudRate: number
  dataBits: number
  parity: Parity
  stopBits: number
  respTimeoutMs: number
  retries: number
  interFrameGapMs: number
}

export interface DeviceDto {
  id: string
  name: string
  channelId: string
  package: string
  slave: number
  online: boolean
  /** 实例级点表覆盖，供设置页回填 */
  overrides: Record<string, unknown>
}

export type DataType = 'u16' | 'i16' | 'u32' | 'i32' | 'u64' | 'f32' | 'f64' | 'bcd16' | 'bcd32'
export type RegArea = 'coil' | 'discrete' | 'input' | 'holding'
export type Access = 'r' | 'rw'

export interface BitDto {
  bit: number
  name: string
  on: string | null
  off: string | null
}

export interface PointDto {
  id: string
  name: string
  register: string
  area: RegArea
  type: DataType
  access: Access
  scale: number | null
  offset: number | null
  unit: string
  bits: BitDto[]
}

export interface PackageDto {
  id: string
  name: string
  vendor: string
  version: string
  driver: string
  description: string
  /** 设备包声明的默认工作区页面（空 = 宿主 fallback 四页） */
  ui: PackageUiDto
  points: PointDto[]
}

export interface PackageUiDto {
  pages: DevicePageDto[]
}

/** 设备工作区的一页：由宿主组件按顺序组合。 */
export interface DevicePageDto {
  id: string
  title: string
  icon: string
  components: DevicePageComponentDto[]
}

export interface DevicePageComponentDto {
  /** overview / dashboard / tags / settings */
  kind: string
  title: string
  config: Record<string, unknown> | null
}

/** 某设备类型的页面覆盖（workspaces.json）。 */
export interface WorkspaceDto {
  deviceType: string
  pages: DevicePageDto[]
}

export interface SerialPortDto {
  portName: string
  hwId: string | null
  description: string
}

export interface WidgetLayout {
  x: number
  y: number
  w: number
  h: number
}

/** 仪表盘组件：kind 决定渲染器，config 是组件专属参数（未来插件 widget 同构）。 */
export interface WidgetDto {
  id: string
  kind: string
  title: string
  tag: string
  unit: string
  config: Record<string, unknown> | null
  layout: WidgetLayout
}

export interface DashboardDto {
  id: string
  name: string
  cols: number
  /** 缺省 = 系统全局看板；device = 某台设备的工作区看板 */
  scopeKind?: string | null
  scopeId?: string | null
  widgets: WidgetDto[]
}

/** UI 插件贡献点：仪表盘组件。渲染 kind 为 `plugin:<插件id>:<组件id>`。 */
export interface UiWidgetDto {
  id: string
  name: string
  description: string
  defaultW: number
  defaultH: number
}

/** UI 插件贡献点：导航页。路由为 `/plugin/<插件id>/<页面id>`。 */
export interface UiPageDto {
  id: string
  title: string
  icon: string
}

/** 设备详情面板贡献点（v1 仅展示，不渲染）。 */
export interface UiDevicePanelDto {
  id: string
  title: string
}

/** 已安装的 ESM UI 插件；入口通过 gatewayd 的 /plugins/<id>/<entry> 提供。 */
export interface UiPluginDto {
  id: string
  name: string
  version: string
  description: string
  author: string
  sdkVersion: number
  permissions: string[]
  entry: string
  widgets: UiWidgetDto[]
  pages: UiPageDto[]
  devicePanels: UiDevicePanelDto[]
}

export interface SnapshotData {
  channels: ChannelDto[]
  devices: DeviceDto[]
  packages: PackageDto[]
  tags: TagChange[]
}

export interface SerialChannelConfig {
  id: string
  name: string
  port: string
  hwId?: string | null
  baudRate: number
  dataBits: number
  parity: Parity
  stopBits: number
  respTimeoutMs: number
  retries: number
  interFrameGapMs: number
}

export interface DeviceConfig {
  id: string
  name: string
  channelId: string
  package: string
  slave: number
  overrides?: Record<string, unknown>
}

export type ClientMsg =
  | { type: 'hello'; token: string }
  | { type: 'getSnapshot' }
  | { type: 'subscribe'; tags: string[] }
  | { type: 'unsubscribe'; tags: string[] }
  | { type: 'writeTag'; tag: string; value: TagValue }
  | { type: 'scanChannel'; channel: string; range?: [number, number] | null }
  | { type: 'getSerialPorts' }
  | { type: 'getPackages' }
  | { type: 'upsertChannel'; config: SerialChannelConfig }
  | { type: 'removeChannel'; channel: string }
  | { type: 'addDevice'; config: DeviceConfig }
  | { type: 'removeDevice'; device: string }
  | { type: 'importPackage'; path: string }
  | { type: 'removePackage'; id: string }
  | { type: 'getDashboards' }
  | { type: 'saveDashboard'; dashboard: DashboardDto }
  | { type: 'removeDashboard'; id: string }
  | { type: 'getUiPlugins' }
  | { type: 'importUiPlugin'; path: string }
  | { type: 'removeUiPlugin'; id: string }
  | { type: 'getWorkspaces' }
  | { type: 'saveWorkspace'; workspace: WorkspaceDto }
  | { type: 'removeWorkspace'; deviceType: string }

export type ServerMsg =
  | { type: 'welcome'; hostVersion: string }
  | ({ type: 'snapshot' } & SnapshotData)
  | { type: 'changes'; items: TagChange[] }
  | { type: 'channelState'; channel: string; state: string; detail: string }
  | { type: 'deviceState'; device: string; online: boolean }
  | { type: 'scanProgress'; channel: string; slave: number; found: boolean }
  | { type: 'scanDone'; channel: string; found: number[] }
  | { type: 'serialPorts'; ports: SerialPortDto[] }
  | { type: 'packages'; packages: PackageDto[] }
  | { type: 'dashboards'; dashboards: DashboardDto[] }
  | { type: 'uiPlugins'; plugins: UiPluginDto[] }
  | { type: 'workspaces'; workspaces: WorkspaceDto[] }
  | { type: 'commandOk'; kind: string }
  | { type: 'error'; message: string }

export type SnapshotMsg = Extract<ServerMsg, { type: 'snapshot' }>
