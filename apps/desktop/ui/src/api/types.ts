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

export interface EnumValueDto {
  value: number
  label: string
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
  /** 写值下限（工程量） */
  min?: number | null
  /** 写值上限（工程量） */
  max?: number | null
  /** 写值步长（工程量） */
  step?: number | null
  /** 枚举写值表（仅整数类型；值按工程量口径） */
  enum?: EnumValueDto[]
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
  /** 包自带的 ESM 小组件（P3） */
  widgets?: PackageWidgetDto[]
}

/** 组件槽位声明（宿主据此生成绑定编辑器/校验写权限）。 */
export interface WidgetBindingDto {
  type: string
  many?: boolean
  write?: boolean
  required?: boolean
}

export interface WidgetSizeDto {
  w: number
  h: number
  minW: number
  minH: number
}

/** 设备包自带的 ESM 小组件；kind = `<包id>:<id>`。 */
export interface PackageWidgetDto {
  id: string
  title: string
  description: string
  entry: string
  size: WidgetSizeDto
  bindings: Record<string, WidgetBindingDto>
  config: Record<string, unknown> | null
  permissions: string[]
}

/** 设备工作区的一页：由宿主组件按顺序组合。 */
export interface DevicePageDto {
  id: string
  title: string
  icon: string
  components: DevicePageComponentDto[]
}

/** 一个槽位的绑定：point（相对点）或 tag（绝对 tag）二选一，bit 只与 point 组合。 */
export interface BindingDecl {
  point?: string | null
  bit?: number | null
  tag?: string | null
}

/** 槽位绑定：单绑定或绑定数组（many 槽位）。 */
export type BindingSpec = BindingDecl | BindingDecl[]

/** 12 列网格位置（0 基，单位 = 格）。 */
export interface WidgetRect {
  x: number
  y: number
  w: number
  h: number
}

export interface DevicePageComponentDto {
  /** v2 组件实例 id（v1 组件缺省） */
  id?: string | null
  /** 核心组件 / `plugin:<插件id>:<组件id>` / `<包id>:<组件短名>` */
  kind: string
  title: string
  /** 槽位 → 绑定；包内页面用相对点，用户页面可用绝对 tag */
  bind?: Record<string, BindingSpec>
  /** 网格位置；整页要么全有、要么全无（流式） */
  rect?: WidgetRect | null
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
  | { type: 'validatePackage'; manifest: Record<string, unknown>; points: Record<string, unknown> }
  | { type: 'installDraftPackage'; manifest: Record<string, unknown>; points: Record<string, unknown> }
  | {
      type: 'exportPackage'
      manifest: Record<string, unknown>
      points: Record<string, unknown>
      dir: string
    }

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
  | { type: 'packageValidation'; errors: string[]; warnings: string[] }
  | { type: 'commandOk'; kind: string }
  | { type: 'error'; message: string }

export type SnapshotMsg = Extract<ServerMsg, { type: 'snapshot' }>
