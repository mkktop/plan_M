import type { WidgetConfigSchema } from '@/api/types'

type Field = WidgetConfigSchema[string]

function accepts(field: Field, value: unknown): boolean {
  if (field.type === 'select') return typeof value === 'string' && !!field.choices?.includes(value)
  if (field.type === 'boolean') return typeof value === 'boolean'
  if (field.type === 'string') return typeof value === 'string'
  return typeof value === 'number' && Number.isFinite(value)
    && (field.type !== 'integer' || Number.isInteger(value))
    && (field.min == null || value >= field.min)
    && (field.max == null || value <= field.max)
}

export function widgetConfigError(schema: WidgetConfigSchema | undefined, values: Record<string, unknown> | null): string | undefined {
  const invalid = Object.entries(schema ?? {}).find(([key, field]) =>
    values && Object.hasOwn(values, key) && !accepts(field, values[key]),
  )
  return invalid ? `「${invalid[1].label || invalid[0]}」的类型或范围不正确` : undefined
}

export function effectiveWidgetConfig(
  schema: WidgetConfigSchema | undefined,
  defaults: Record<string, unknown> | null,
  override: Record<string, unknown> | null | undefined,
): Record<string, unknown> {
  const base: Record<string, unknown> = { ...defaults }
  for (const [key, field] of Object.entries(schema ?? {})) {
    if (base[key] === undefined && field.default != null) base[key] = field.default
  }
  for (const [key, value] of Object.entries(override ?? {})) {
    const field = schema && Object.hasOwn(schema, key) ? schema[key] : undefined
    if (!field || accepts(field, value)) Object.defineProperty(base, key, { value, enumerable: true, writable: true, configurable: true })
  }
  return base
}
