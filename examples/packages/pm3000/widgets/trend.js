// 示例设备包 ESM 小组件：多点实时趋势。
//
// 宿主契约（v2）：
//   export function mount(host) -> dispose
//   host.el        已挂载在 ShadowRoot 内的容器
//   host.slots.sub('series', cb)  槽位样本数组（PointMeta + TagSample）
//   host.slots.read('series')     最后一帧（mount 时同步渲染，避免闪空）
//   host.config    页面里声明的 config（windowSec 等）
//
// 组件只通过槽位拿数据，不接触 tag 寻址；订阅/去重/质量戳全部由宿主负责。
// 样式仅使用宿主注入的 CSS 变量，不污染全局。

const NS = 'http://www.w3.org/2000/svg'
const PALETTE = ['#4db5ff', '#35d399', '#f5b942', '#c084fc', '#ff8a4c', '#67e8f9']

function numeric(sample) {
  const v = sample && sample.sample && sample.sample.value
  if (!v) return null
  if (typeof v.f64 === 'number') return v.f64
  if (typeof v.i64 === 'number') return v.i64
  if (typeof v.bool === 'boolean') return v.bool ? 1 : 0
  return null
}

function fmt(v) {
  if (!Number.isFinite(v)) return '—'
  const abs = Math.abs(v)
  if (abs >= 1000) return v.toFixed(0)
  if (abs >= 100) return v.toFixed(1)
  return v.toFixed(abs < 10 ? 2 : 1)
}

function windowSec(config) {
  const n = Number(config && config.windowSec)
  if (!Number.isFinite(n)) return 60
  return Math.max(10, Math.min(3600, n))
}

export function mount(host) {
  const doc = host.el.ownerDocument
  const span = windowSec(host.config)
  const series = new Map() // tag -> { meta, points: [{t, v}], color }
  let order = []

  const style = doc.createElement('style')
  style.textContent = `
    .trend { display: flex; flex-direction: column; height: 100%; min-height: 0; gap: 6px;
      color: var(--pm-color-text, #e8eef7); font-family: var(--pm-font, inherit); }
    .trend-head { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; padding: 0 2px; }
    .trend-legend { display: inline-flex; align-items: center; gap: 4px; font-size: 10.5px;
      color: var(--pm-color-text-muted, #8494a8); white-space: nowrap; }
    .trend-dot { width: 7px; height: 7px; border-radius: 50%; display: inline-block; }
    .trend-last { font-family: var(--pm-font-mono, monospace); font-size: 11px;
      color: var(--pm-color-text, #e8eef7); }
    .trend-plot { position: relative; flex: 1; min-height: 0; border-radius: 6px;
      background: var(--pm-color-panel, rgba(255,255,255,0.03)); overflow: hidden; }
    .trend-plot svg { display: block; width: 100%; height: 100%; }
    .trend-grid { stroke: var(--pm-color-border, #26313f); stroke-width: 1; opacity: 0.55; }
    .trend-axis { fill: var(--pm-color-text-muted, #8494a8); font-size: 9px;
      font-family: var(--pm-font-mono, monospace); }
    .trend-empty { position: absolute; inset: 0; display: flex; align-items: center;
      justify-content: center; color: var(--pm-color-text-muted, #8494a8); font-size: 11px; }
    .trend-bad { color: var(--pm-color-bad, #f5b942); }
  `

  const wrap = doc.createElement('div')
  wrap.className = 'trend'
  const head = doc.createElement('div')
  head.className = 'trend-head'
  const plot = doc.createElement('div')
  plot.className = 'trend-plot'
  const svg = doc.createElementNS(NS, 'svg')
  svg.setAttribute('viewBox', '0 0 600 200')
  svg.setAttribute('preserveAspectRatio', 'none')

  const empty = doc.createElement('div')
  empty.className = 'trend-empty'
  empty.textContent = '等待数据…'

  plot.append(svg, empty)
  wrap.append(head, plot)
  host.el.append(style, wrap)

  let raf = 0
  function schedule() {
    if (raf) return
    raf = requestAnimationFrame(() => {
      raf = 0
      draw()
    })
  }

  function ingest(samples) {
    const now = Date.now()
    for (const s of samples || []) {
      const meta = s.meta || {}
      const key = meta.tag || meta.key || String(series.size)
      let entry = series.get(key)
      if (!entry) {
        entry = {
          meta,
          color: PALETTE[series.size % PALETTE.length],
          points: [],
          last: null,
        }
        series.set(key, entry)
        order = [...series.values()]
      }
      const v = numeric(s)
      if (v !== null) {
        entry.last = v
        entry.points.push({ t: now, v })
      }
      const cutoff = now - span * 1000
      while (entry.points.length && entry.points[0].t < cutoff) entry.points.shift()
    }
    schedule()
  }

  function draw() {
    head.replaceChildren()
    let min = Infinity
    let max = -Infinity
    for (const entry of order) {
      for (const p of entry.points) {
        if (p.v < min) min = p.v
        if (p.v > max) max = p.v
      }
    }
    if (!Number.isFinite(min)) {
      min = 0
      max = 1
    } else if (min === max) {
      min -= 1
      max += 1
    }

    for (const entry of order) {
      const legend = doc.createElement('span')
      legend.className = 'trend-legend'
      const dot = doc.createElement('span')
      dot.className = 'trend-dot'
      dot.style.background = entry.color
      const label = doc.createElement('span')
      label.textContent = entry.meta.name || entry.meta.key || entry.meta.tag || '?'
      const last = doc.createElement('span')
      last.className = 'trend-last'
      last.textContent = entry.last === null ? '—' : fmt(entry.last)
      if (entry.meta.unit) last.textContent += ` ${entry.meta.unit}`
      legend.append(dot, label, last)
      head.append(legend)
    }

    svg.replaceChildren()
    const grid = doc.createElementNS(NS, 'path')
    grid.setAttribute('class', 'trend-grid')
    grid.setAttribute('d', 'M0 50H600M0 100H600M0 150H600M150 0V200M300 0V200M450 0V200')
    svg.append(grid)

    const now = Date.now()
    const t0 = now - span * 1000
    const yOf = (v) => 196 - ((v - min) / (max - min)) * 188
    const xOf = (t) => ((t - t0) / (span * 1000)) * 600

    for (const entry of order) {
      if (entry.points.length < 2) continue
      const d = entry.points
        .map((p, i) => `${i === 0 ? 'M' : 'L'}${xOf(p.t).toFixed(1)} ${yOf(p.v).toFixed(1)}`)
        .join(' ')
      const path = doc.createElementNS(NS, 'path')
      path.setAttribute('d', d)
      path.setAttribute('fill', 'none')
      path.setAttribute('stroke', entry.color)
      path.setAttribute('stroke-width', '1.8')
      path.setAttribute('stroke-linejoin', 'round')
      path.setAttribute('stroke-linecap', 'round')
      svg.append(path)
    }

    const top = doc.createElementNS(NS, 'text')
    top.setAttribute('class', 'trend-axis')
    top.setAttribute('x', '6')
    top.setAttribute('y', '13')
    top.textContent = fmt(max)
    const bottom = doc.createElementNS(NS, 'text')
    bottom.setAttribute('class', 'trend-axis')
    bottom.setAttribute('x', '6')
    bottom.setAttribute('y', '193')
    bottom.textContent = fmt(min)
    svg.append(top, bottom)

    empty.style.display = order.some((e) => e.points.length) ? 'none' : 'flex'
  }

  const off = host.slots.sub('series', ingest)
  const initial = host.slots.read('series')
  if (initial && initial.length) ingest(initial)
  draw()

  return () => {
    off()
    if (raf) cancelAnimationFrame(raf)
    host.el.replaceChildren()
  }
}
