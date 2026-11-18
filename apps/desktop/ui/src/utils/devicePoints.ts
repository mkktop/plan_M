import type { DeviceDto, PointDto, TagValue } from '@/api/types'

export function pointCount(points?: PointDto[]): number {
  return points?.reduce((count, point) => count + 1 + point.bits.length, 0) ?? 0
}

export function effectivePoint(device: DeviceDto | undefined, point: PointDto): PointDto {
  const prefix = `points.${point.id}.`
  const scale = device?.overrides?.[`${prefix}scale`]
  const offset = device?.overrides?.[`${prefix}offset`]
  const pollGroup = device?.overrides?.[`${prefix}pollGroup`]
  return {
    ...point,
    scale: typeof scale === 'number' ? scale : point.scale,
    offset: typeof offset === 'number' ? offset : point.offset,
    pollGroup: typeof pollGroup === 'string' ? pollGroup : point.pollGroup,
  }
}

export function numericTagValue(point: PointDto, value: number): TagValue {
  if (point.area === 'coil' || point.area === 'discrete') return { bool: value !== 0 }
  if (!Number.isSafeInteger(value) || point.type === 'f32' || point.type === 'f64') return { f64: value }
  return { i64: value }
}
