//! 设备工作区页面：宿主组件目录 + 包默认/内置 fallback 页面。
//!
//! 组件 kind 白名单必须与 `crates/packaging/src/validate.rs` 的
//! `ALLOWED_DEVICE_PAGE_COMPONENTS` 保持一致。

import type { DevicePageDto, PackageDto, WorkspaceDto } from '@/api/types'

export interface HostDeviceComponent {
  kind: string
  label: string
  icon: string
  description: string
}

export const HOST_DEVICE_COMPONENTS: HostDeviceComponent[] = [
  {
    kind: 'overview',
    label: '设备概览',
    icon: 'pulse',
    description: '在线状态、通道/从站、质量统计与最近更新',
  },
  {
    kind: 'dashboard',
    label: '仪表盘',
    icon: 'layout',
    description: '当前设备的组件画布（每个实例独立布局）',
  },
  {
    kind: 'tags',
    label: '实时点表',
    icon: 'grid',
    description: '当前设备的实时值、质量与写值',
  },
  {
    kind: 'settings',
    label: '参数设置',
    icon: 'settings',
    description: '设备实例名称、通道、从站与 overrides',
  },
]

export const ALLOWED_DEVICE_COMPONENT_KINDS = HOST_DEVICE_COMPONENTS.map((c) => c.kind)

const DEFAULT_DEVICE_PAGES: DevicePageDto[] = [
  {
    id: 'overview',
    title: '设备概览',
    icon: 'pulse',
    components: [{ kind: 'overview', title: '', config: null }],
  },
  {
    id: 'dashboard',
    title: '仪表盘',
    icon: 'layout',
    components: [{ kind: 'dashboard', title: '', config: null }],
  },
  {
    id: 'tags',
    title: '实时点表',
    icon: 'grid',
    components: [{ kind: 'tags', title: '', config: null }],
  },
  {
    id: 'settings',
    title: '参数设置',
    icon: 'settings',
    components: [{ kind: 'settings', title: '', config: null }],
  },
]

/** 深拷贝，避免编辑器原地修改包/默认数据。 */
export function clonePages(pages: DevicePageDto[]): DevicePageDto[] {
  return JSON.parse(JSON.stringify(pages)) as DevicePageDto[]
}

/** 用户覆盖 > 设备包声明 > 宿主内置四页。 */
export function pagesForPackage(
  pkg?: PackageDto | null,
  saved?: WorkspaceDto,
): DevicePageDto[] {
  const source =
    saved?.pages?.length
      ? saved.pages
      : pkg?.ui?.pages?.length
        ? pkg.ui.pages
        : DEFAULT_DEVICE_PAGES
  return clonePages(source)
}

export function componentLabel(kind: string): string {
  return HOST_DEVICE_COMPONENTS.find((c) => c.kind === kind)?.label ?? kind
}

export function isKnownComponent(kind: string): boolean {
  return ALLOWED_DEVICE_COMPONENT_KINDS.includes(kind)
}
