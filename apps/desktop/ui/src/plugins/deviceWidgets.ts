//! 设备页组件注册表：核心组件 + UI 插件 widget + 设备包 ESM widget 统一寻址。
//!
//! kind 形态：
//! - 核心：`core:value` 等（裸名是 v1 别名）
//! - 插件：`plugin:<插件id>:<组件id>`（模块入口 = 插件 entry）
//! - 设备包：`<包id>:<组件短名>`（模块入口 = `/packages/<id>/<version>/<entry>`）

import {
  gatewayHttpBase,
  pluginHost,
  pluginModuleUrl,
  pluginWidgetInfo,
  pluginWidgetOptions,
} from '@/plugins/host'
import { HOST_DEVICE_COMPONENTS, normalizeComponentKind } from '@/utils/devicePages'
import type { PackageDto, PackageWidgetDto, UiWidgetDto, WidgetBindingDto } from '@/api/types'

export interface ExternalWidgetDescriptor {
  kind: string
  title: string
  description: string
  source: 'plugin' | 'package'
  providerId: string
  providerName: string
  version: string
  /** 包内/插件内相对入口（调试展示用） */
  entry: string
  moduleUrl: string
  size: { w: number; h: number; minW: number; minH: number }
  bindings: Record<string, WidgetBindingDto>
  config: Record<string, unknown> | null
  permissions: string[]
}

export function packageWidgetKind(packageId: string, widgetId: string): string {
  return `${packageId}:${widgetId}`
}

/** 设备包组件入口 URL：`/packages/<id>/<version>/<entry>`（gatewayd 静态路由）。 */
export function packageWidgetModuleUrl(
  pkg: Pick<PackageDto, 'id' | 'version'>,
  entry: string,
): string {
  const path = entry
    .split(/[\\/]+/)
    .filter(Boolean)
    .map(encodeURIComponent)
    .join('/')
  return `${gatewayHttpBase()}/packages/${encodeURIComponent(pkg.id)}/${encodeURIComponent(
    pkg.version,
  )}/${path}?v=${encodeURIComponent(pkg.version)}`
}

function packageDescriptor(
  pkg: PackageDto,
  widget: PackageWidgetDto,
): ExternalWidgetDescriptor {
  return {
    kind: packageWidgetKind(pkg.id, widget.id),
    title: widget.title || widget.id,
    description: widget.description,
    source: 'package',
    providerId: pkg.id,
    providerName: pkg.name,
    version: pkg.version,
    entry: widget.entry,
    moduleUrl: packageWidgetModuleUrl(pkg, widget.entry),
    size: {
      w: widget.size?.w ?? 6,
      h: widget.size?.h ?? 3,
      minW: widget.size?.minW ?? 2,
      minH: widget.size?.minH ?? 1,
    },
    bindings: widget.bindings ?? {},
    config: widget.config ?? null,
    permissions: widget.permissions ?? [],
  }
}

function pluginDescriptor(
  dto: { id: string; name: string; version: string; entry: string; permissions: string[] },
  widget: UiWidgetDto,
  kind: string,
): ExternalWidgetDescriptor {
  return {
    kind,
    title: widget.name || widget.id,
    description: widget.description,
    source: 'plugin',
    providerId: dto.id,
    providerName: dto.name,
    version: dto.version,
    entry: dto.entry,
    moduleUrl: pluginModuleUrl(dto),
    size: {
      w: widget.defaultW || 3,
      h: widget.defaultH || 2,
      minW: 2,
      minH: 1,
    },
    // v1 插件只认识一个 value 槽位；v2 插件（mountWidget）可按需扩展。
    bindings: { value: { type: 'point', required: true } },
    config: null,
    permissions: dto.permissions ?? [],
  }
}

/** 解析一个外部组件 kind（插件或设备包）；找不到返回 null。 */
export function resolveExternalWidget(
  kind: string,
  pkg?: PackageDto | null,
): ExternalWidgetDescriptor | null {
  void pluginHost.version
  if (kind.startsWith('plugin:')) {
    const hit = pluginWidgetInfo(kind)
    if (!hit) return null
    return pluginDescriptor(hit.plugin, hit.widget, kind)
  }
  const i = kind.indexOf(':')
  if (i <= 0) return null
  const packageId = kind.slice(0, i)
  const widgetId = kind.slice(i + 1)
  if (!pkg || pkg.id !== packageId) return null
  const widget = (pkg.ui.widgets ?? []).find((w) => w.id === widgetId)
  return widget ? packageDescriptor(pkg, widget) : null
}

export interface DeviceWidgetOption {
  kind: string
  label: string
  description: string
  core: boolean
  external?: ExternalWidgetDescriptor
}

/** 设备页编辑器可选组件：核心 + 当前设备包的 ESM 组件 + 已加载插件的组件。 */
export function deviceWidgetOptions(pkg?: PackageDto | null): DeviceWidgetOption[] {
  void pluginHost.version
  const options: DeviceWidgetOption[] = HOST_DEVICE_COMPONENTS.map((c) => ({
    kind: c.kind,
    label: `${c.label} · ${c.kind}`,
    description: c.description,
    core: true,
  }))
  for (const widget of pkg?.ui?.widgets ?? []) {
    const d = packageDescriptor(pkg as PackageDto, widget)
    options.push({
      kind: d.kind,
      label: `${d.title} · ${d.providerName}`,
      description: d.description,
      core: false,
      external: d,
    })
  }
  for (const plugin of pluginHost.statuses) {
    if (plugin.error) continue
    const prefix = `plugin:${plugin.id}:`
    for (const option of pluginWidgetOptions()) {
      if (!option.kind.startsWith(prefix)) continue
      const d = resolveExternalWidget(option.kind, pkg)
      if (!d) continue
      options.push({
        kind: d.kind,
        label: `${d.title} · ${d.providerName}`,
        description: d.description,
        core: false,
        external: d,
      })
    }
  }
  return options
}

/** 组件默认尺寸：外部组件读声明，核心组件走内置表。 */
export function widgetDefaultSize(
  kind: string,
  pkg?: PackageDto | null,
): { w: number; h: number } {
  const external = resolveExternalWidget(kind, pkg)
  if (external) return { w: external.size.w, h: external.size.h }
  return coreDefaultSize(normalizeComponentKind(kind))
}

function coreDefaultSize(kind: string): { w: number; h: number } {
  switch (kind) {
    case 'core:input':
    case 'core:bits':
      return { w: 4, h: 2 }
    case 'core:group':
      return { w: 6, h: 3 }
    case 'core:dashboard':
    case 'core:tags':
      return { w: 12, h: 10 }
    case 'core:overview':
      return { w: 12, h: 6 }
    case 'core:settings':
      return { w: 12, h: 8 }
    default:
      return { w: 3, h: 2 }
  }
}
