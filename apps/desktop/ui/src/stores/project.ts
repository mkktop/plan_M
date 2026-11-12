import { computed, reactive, ref } from 'vue'
import { defineStore } from 'pinia'
import { gateway } from '@/api/ws'
import type {
  ChannelConfig,
  ChannelDto,
  ClientMsg,
  DeviceAddress,
  DeviceConfig,
  DeviceDto,
  DriverInfoDto,
  PackageDto,
  ScanRange,
  SnapshotData,
  SnapshotMsg,
  TransportKind,
  TransportTarget,
} from '@/api/types'

/** 工程快照：通道/设备/设备包。业务真值在 gatewayd，这里只是最近一次快照的视图。 */
export const useProjectStore = defineStore('project', () => {
  const channels = ref<ChannelDto[]>([])
  const devices = ref<DeviceDto[]>([])
  const packages = ref<PackageDto[]>([])
  const channelStates = reactive<Record<string, string>>({})
  /** 宿主能力（Welcome 下发） */
  const apiVersion = ref(0)
  const transports = ref<TransportKind[]>([])
  const drivers = ref<DriverInfoDto[]>([])
  const targets = reactive<Partial<Record<TransportKind, TransportTarget[]>>>({})
  const loaded = ref(false)
  const refreshing = ref(false)

  const packageById = computed(() => {
    const m = new Map<string, PackageDto>()
    for (const p of packages.value) m.set(p.id, p)
    return m
  })

  const onlineDeviceCount = computed(() => devices.value.filter((d) => d.online).length)
  const totalPointCount = computed(() =>
    packages.value.reduce(
      (n, p) => n + p.points.reduce((m, pt) => m + 1 + pt.bits.length, 0),
      0,
    ),
  )

  function applyWelcome(msg: {
    apiVersion: number
    transports: TransportKind[]
    drivers: DriverInfoDto[]
  }): void {
    apiVersion.value = msg.apiVersion
    transports.value = msg.transports
    drivers.value = msg.drivers
  }

  function applySnapshot(s: SnapshotData): void {
    channels.value = s.channels
    devices.value = s.devices
    packages.value = s.packages
    for (const c of s.channels) channelStates[c.id] = c.state
    // 清掉已删除通道的残留状态
    for (const id of Object.keys(channelStates)) {
      if (!s.channels.some((c) => c.id === id)) delete channelStates[id]
    }
    loaded.value = true
  }

  function applyPackages(list: PackageDto[]): void {
    packages.value = list
  }

  function applyChannelState(channel: string, state: string): void {
    if (channel) channelStates[channel] = state
  }

  function applyDeviceState(deviceId: string, online: boolean): void {
    const d = devices.value.find((x) => x.id === deviceId)
    if (d) d.online = online
  }

  function devicesOn(channelId: string): DeviceDto[] {
    return devices.value.filter((d) => d.channelId === channelId)
  }

  function packageOf(device?: DeviceDto | null): PackageDto | undefined {
    return device ? packageById.value.get(device.package) : undefined
  }

  function channelOf(device?: DeviceDto | null): ChannelDto | undefined {
    return device ? channels.value.find((c) => c.id === device.channelId) : undefined
  }

  function isTransportSupported(kind: TransportKind): boolean {
    return transports.value.includes(kind)
  }

  function driversForTransport(kind: TransportKind): DriverInfoDto[] {
    return drivers.value.filter((d) => d.transports.includes(kind))
  }

  async function refresh(): Promise<void> {
    if (refreshing.value) return
    refreshing.value = true
    try {
      const r = await gateway.request<SnapshotMsg>({ type: 'getSnapshot' }, ['snapshot'])
      applySnapshot(r)
    } finally {
      refreshing.value = false
    }
  }

  async function command(msg: ClientMsg): Promise<void> {
    const r = await gateway.request<SnapshotMsg>(msg, ['snapshot', 'error'])
    applySnapshot(r)
  }

  async function loadTransportTargets(kind: TransportKind): Promise<TransportTarget[]> {
    const r = await gateway.request<{
      type: 'transportTargets'
      transport: TransportKind
      targets: TransportTarget[]
    }>({ type: 'getTransportTargets', transport: kind }, ['transportTargets', 'error'])
    targets[kind] = r.targets
    return r.targets
  }

  /** 兼容旧调用：串口候选列表。 */
  async function loadSerialPorts(): Promise<TransportTarget[]> {
    return loadTransportTargets('serial')
  }

  async function loadPackages(): Promise<PackageDto[]> {
    const r = await gateway.request<{ type: 'packages'; packages: PackageDto[] }>(
      { type: 'getPackages' },
      ['packages', 'error'],
    )
    packages.value = r.packages
    return r.packages
  }

  async function scanChannel(
    channel: string,
    range: ScanRange,
    driver?: string | null,
  ): Promise<DeviceAddress[]> {
    // 扫描是串行探测，最坏情况几十秒（247 × 超时 × 重试），超时放宽。
    const r = await gateway.request<{
      type: 'scanDone'
      channel: string
      found: DeviceAddress[]
    }>({ type: 'scanChannel', channel, driver: driver ?? null, range }, ['scanDone', 'error'], 180_000)
    return r.found
  }

  async function upsertChannel(config: ChannelConfig): Promise<void> {
    await command({ type: 'upsertChannel', config })
  }

  async function removeChannel(channel: string): Promise<void> {
    await command({ type: 'removeChannel', channel })
  }

  async function addDevice(config: DeviceConfig): Promise<void> {
    await command({ type: 'addDevice', config })
  }

  async function removeDevice(device: string): Promise<void> {
    await command({ type: 'removeDevice', device })
  }

  async function importPackage(path: string): Promise<void> {
    await command({ type: 'importPackage', path })
  }

  async function removePackage(id: string): Promise<void> {
    await command({ type: 'removePackage', id })
  }

  return {
    channels,
    devices,
    packages,
    channelStates,
    apiVersion,
    transports,
    drivers,
    targets,
    loaded,
    refreshing,
    packageById,
    onlineDeviceCount,
    totalPointCount,
    applyWelcome,
    applySnapshot,
    applyPackages,
    applyChannelState,
    applyDeviceState,
    devicesOn,
    packageOf,
    channelOf,
    isTransportSupported,
    driversForTransport,
    refresh,
    loadTransportTargets,
    loadSerialPorts,
    loadPackages,
    scanChannel,
    upsertChannel,
    removeChannel,
    addDevice,
    removeDevice,
    importPackage,
    removePackage,
  }
})
