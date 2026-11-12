import type {
  ChannelConfig,
  ChannelDto,
  DeviceAddress,
  DeviceDto,
  DriverInfoDto,
  PackageDto,
  Parity,
  TransportKind,
  TransportTarget,
} from '@/api/types'

export const TRANSPORT_LABELS: Record<TransportKind, string> = {
  serial: '串口 / RS485',
  tcp: 'TCP 网关',
  sim: '虚拟总线',
  ble: '蓝牙 BLE',
}

export function transportLabel(kind: TransportKind): string {
  return TRANSPORT_LABELS[kind] ?? kind
}

export function parityLetter(p: Parity): string {
  return p === 'none' ? 'N' : p === 'odd' ? 'O' : 'E'
}

/** 通道一句话摘要（列表/设备页用）。 */
export function channelSummary(ch: ChannelDto | ChannelConfig): string {
  switch (ch.transport) {
    case 'serial':
      return `${ch.port} · ${ch.baudRate} ${ch.dataBits}${parityLetter(ch.parity)}${ch.stopBits}`
    case 'tcp':
      return `${ch.host}:${ch.port}`
    case 'sim':
      return '虚拟总线（演示）'
    case 'ble':
      return ch.address
    default:
      return '—'
  }
}

/** 设备地址显示：Modbus 数字 → #1，BLE/文本 → 原样。 */
export function addressLabel(address: DeviceAddress): string {
  return typeof address === 'number' ? `#${address}` : address
}

/** 点地址显示：寄存器字符串原样，对象转 JSON。 */
export function addressText(address: string | Record<string, unknown>): string {
  return typeof address === 'string' ? address : JSON.stringify(address)
}

export function deviceAddressLabel(device: DeviceDto): string {
  return addressLabel(device.address)
}

export function targetLabel(target: TransportTarget): string {
  const desc = target.description ? ` · ${target.description}` : ''
  return `${target.label}${desc}`
}

/** 设备包能否跑在指定传输上（驱动能力 ∩ 包声明）。 */
export function packageSupportsTransport(
  pkg: PackageDto,
  kind: TransportKind,
  drivers: DriverInfoDto[],
): boolean {
  // Welcome 尚未到达时不做过滤，避免表单短暂空白
  if (!drivers.length) return true
  const driver = drivers.find((d) => d.id === pkg.protocol.driver)
  if (!driver) return false
  const driverOk = driver.transports.includes(kind)
  const packageOk =
    pkg.protocol.transports.length === 0 || pkg.protocol.transports.includes(kind)
  return driverOk && packageOk
}

export function driverName(id: string, drivers: DriverInfoDto[]): string {
  return drivers.find((d) => d.id === id)?.name ?? id
}
