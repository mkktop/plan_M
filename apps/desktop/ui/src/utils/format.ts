import type { PointDto, Quality, TagValue } from '@/api/types'

/** 浮点收敛尾差：最多 6 位小数、去尾零（220.10000000000002 → "220.1"）。 */
export function formatNumber(n: number, maxDecimals = 6): string {
  if (!Number.isFinite(n)) return '—'
  if (Math.abs(n) >= 1e12) return n.toExponential(3)
  const s = n.toFixed(maxDecimals)
  return s.includes('.') ? s.replace(/0+$/, '').replace(/\.$/, '') : s
}

export function primitiveOf(v?: TagValue): number | boolean | string | undefined {
  if (!v) return undefined
  if ('f64' in v) return v.f64
  if ('i64' in v) return v.i64
  if ('bool' in v) return v.bool
  if ('str' in v) return v.str
  return undefined
}

export function valueText(v?: TagValue): string {
  const p = primitiveOf(v)
  if (p === undefined) return '—'
  if (typeof p === 'number') return formatNumber(p)
  if (typeof p === 'boolean') return p ? 'ON' : 'OFF'
  return p
}

export function boolOf(v?: TagValue): boolean | undefined {
  const p = primitiveOf(v)
  return typeof p === 'boolean' ? p : typeof p === 'number' ? p !== 0 : undefined
}

const QUALITY_LABEL: Record<Quality, string> = {
  good: '良好',
  bad: '异常',
  uncertain: '可疑',
  comm_lost: '失联',
}

export function qualityLabel(q?: Quality): string {
  return q ? QUALITY_LABEL[q] ?? q : '—'
}

/** 与 theme.css 中的 .q-* 类对应。 */
export function qualityClass(q?: Quality): string {
  return q ? `q-${q}` : 'q-none'
}

export function formatClock(ts: number): string {
  const d = new Date(ts)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
}

/** 相对时间：<1s 刚刚，<60s "1.2 s"，其余 "3m 05s"。 */
export function formatAge(ms: number): string {
  if (!Number.isFinite(ms)) return '—'
  if (ms < 1000) return '刚刚'
  if (ms < 60_000) {
    const s = ms / 1000
    return `${s < 10 ? s.toFixed(1) : Math.round(s)} s`
  }
  const total = Math.floor(ms / 1000)
  const m = Math.floor(total / 60)
  const s = total % 60
  if (m < 60) return `${m}m ${String(s).padStart(2, '0')}s`
  const h = Math.floor(m / 60)
  return `${h}h ${String(m % 60).padStart(2, '0')}m`
}

export function formatUptime(ms: number): string {
  if (ms <= 0) return '—'
  const total = Math.floor(ms / 1000)
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  if (h > 0) return `${h}h ${String(m).padStart(2, '0')}m`
  if (m > 0) return `${m}m ${String(s).padStart(2, '0')}s`
  return `${s}s`
}

export function typeLabel(p: PointDto): string {
  if (p.area === 'coil' || p.area === 'discrete') return 'BOOL'
  return p.type.toUpperCase()
}

/** 点的显示名兜底（包未安装/点被删除时用 id）。 */
export function safePointName(id: string): string {
  return id || '未命名点'
}

export function isBitArea(area?: string | null): boolean {
  return area === 'coil' || area === 'discrete'
}
