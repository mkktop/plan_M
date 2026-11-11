// 示例 UI 插件：零依赖、纯 ESM、框架无关。
//
// 宿主契约：
//   export function mount(el, ctx, props) -> cleanup? | void
//   props.kind === 'widget'：props.widget = { id, title, tag, unit, config }，另有 props.editing
//   props.kind === 'page'  ：props.pageId / props.title
//
// 插件不自己开 WebSocket：ctx.tags.subscribe 由宿主按 tag 聚合去重后统一订阅。

const STYLE_ID = 'meter-card-plugin-style'

const STYLE = `
.mcard {
  display: flex;
  flex-direction: column;
  gap: 6px;
  height: 100%;
  padding: 2px 2px 0;
  min-height: 0;
  color: var(--text-1, #e8eef7);
  font-family: inherit;
}
.mcard.is-editing { pointer-events: none; }
.mcard-top { display: flex; align-items: baseline; gap: 6px; min-width: 0; }
.mcard-value {
  font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
  font-size: 30px;
  line-height: 1;
  font-weight: 650;
  letter-spacing: -0.02em;
  color: var(--text-1, #e8eef7);
}
.mcard-value.q-good { color: #35d399; }
.mcard-value.q-bad { color: #f5b942; }
.mcard-value.q-uncertain { color: #4db5ff; }
.mcard-value.q-comm_lost, .mcard-value.q-none { color: #7c8c9f; }
.mcard-unit { font-size: 12px; color: var(--text-3, #7c8c9f); }
.mcard-bar {
  position: relative;
  height: 6px;
  border-radius: 99px;
  background: rgba(255, 255, 255, 0.07);
  overflow: hidden;
}
.mcard-fill {
  position: absolute;
  inset: 0 auto 0 0;
  width: 0%;
  border-radius: inherit;
  background: linear-gradient(90deg, #4db5ff, #35d399);
  transition: width 0.25s ease, background 0.2s ease;
}
.mcard-fill.q-bad { background: linear-gradient(90deg, #f5b942, #ff8a4c); }
.mcard-fill.q-comm_lost, .mcard-fill.q-none { background: #445266; }
.mcard-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  margin-top: auto;
  font-size: 10px;
  color: var(--text-3, #7c8c9f);
}
.mcard-quality {
  padding: 1px 6px;
  border-radius: 99px;
  background: rgba(255, 255, 255, 0.05);
  font-size: 9px;
  letter-spacing: 0.04em;
}
.mcard-quality.q-good { color: #35d399; background: rgba(53, 211, 153, 0.1); }
.mcard-quality.q-bad { color: #f5b942; background: rgba(245, 185, 66, 0.1); }
.mcard-quality.q-comm_lost { color: #7c8c9f; }
.mcard-missing { color: var(--text-3, #7c8c9f); font-size: 11px; }
.mcard-page { display: flex; flex-direction: column; gap: 14px; }
.mcard-page h3 { margin: 0; font-size: 15px; }
.mcard-page p { margin: 0; color: var(--text-2, #c2cedd); font-size: 12.5px; line-height: 1.7; }
.mcard-page code {
  padding: 1px 5px;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.06);
  font-family: ui-monospace, Consolas, monospace;
  font-size: 11px;
}
.mcard-page-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 10px;
}
.mcard-page-cell {
  padding: 10px 12px;
  border: 1px solid var(--line, #222d3a);
  border-radius: 8px;
  background: var(--bg-2, #141b24);
}
.mcard-page-cell span { display: block; color: var(--text-3, #7c8c9f); font-size: 10px; text-transform: uppercase; letter-spacing: 0.06em; }
.mcard-page-cell strong { display: block; margin-top: 3px; font-size: 13px; }
.mcard-btn {
  align-self: flex-start;
  padding: 6px 12px;
  border: 1px solid rgba(77, 181, 255, 0.4);
  border-radius: 6px;
  background: rgba(77, 181, 255, 0.12);
  color: #9ad4ff;
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}
.mcard-btn:hover { background: rgba(77, 181, 255, 0.2); }
`

const QUALITY_LABEL = {
  good: 'GOOD',
  bad: 'BAD',
  uncertain: 'UNCERTAIN',
  comm_lost: 'COMM LOST',
}

function ensureStyles() {
  if (document.getElementById(STYLE_ID)) return
  const style = document.createElement('style')
  style.id = STYLE_ID
  style.textContent = STYLE
  document.head.appendChild(style)
}

export function mount(el, ctx, props) {
  ensureStyles()
  if (props && props.kind === 'page') return mountPage(el, ctx, props)
  return mountWidget(el, ctx, props || {})
}

function primitive(value) {
  if (!value) return undefined
  if ('f64' in value) return value.f64
  if ('i64' in value) return value.i64
  if ('bool' in value) return value.bool
  if ('str' in value) return value.str
  return undefined
}

function formatNumber(n, decimals) {
  if (!Number.isFinite(n)) return '—'
  const s = n.toFixed(Math.max(0, Math.min(6, decimals)))
  return s.includes('.') ? s.replace(/0+$/, '').replace(/\.$/, '') : s
}

function mountWidget(el, ctx, props) {
  const w = props.widget || {}
  const config = w.config && typeof w.config === 'object' ? w.config : {}
  const min = Number.isFinite(config.min) ? config.min : 0
  const max = Number.isFinite(config.max) ? config.max : 100
  const decimals = Number.isInteger(config.decimals)
    ? config.decimals
    : Math.abs(max - min) >= 100
      ? 0
      : 1

  const root = document.createElement('div')
  root.className = 'mcard' + (props.editing ? ' is-editing' : '')
  root.innerHTML = `
    <div class="mcard-top">
      <span class="mcard-value q-none">—</span>
      <span class="mcard-unit"></span>
    </div>
    <div class="mcard-bar"><div class="mcard-fill q-none"></div></div>
    <div class="mcard-foot">
      <span class="mcard-quality q-none">WAITING</span>
      <span class="mcard-range"></span>
    </div>`
  el.appendChild(root)

  const valueEl = root.querySelector('.mcard-value')
  const unitEl = root.querySelector('.mcard-unit')
  const fillEl = root.querySelector('.mcard-fill')
  const qualityEl = root.querySelector('.mcard-quality')
  const rangeEl = root.querySelector('.mcard-range')

  unitEl.textContent = w.unit || ''
  rangeEl.textContent = `${formatNumber(min, decimals)} – ${formatNumber(max, decimals)}`

  if (!w.tag) {
    valueEl.textContent = '—'
    qualityEl.textContent = '未绑定点位'
    valueEl.classList.add('mcard-missing')
    return () => {}
  }

  const render = (change) => {
    const primitiveValue = primitive(change.value)
    const q = change.quality || 'none'
    valueEl.classList.remove('q-none', 'q-good', 'q-bad', 'q-uncertain', 'q-comm_lost')
    valueEl.classList.add(`q-${q}`)
    qualityEl.className = `mcard-quality q-${q}`
    qualityEl.textContent = QUALITY_LABEL[q] || q.toUpperCase()
    fillEl.className = `mcard-fill q-${q}`

    if (typeof primitiveValue === 'number') {
      valueEl.textContent = formatNumber(primitiveValue, decimals)
      const span = max - min
      const pct = span === 0 ? 0 : Math.max(0, Math.min(100, ((primitiveValue - min) / span) * 100))
      fillEl.style.width = `${pct}%`
    } else if (typeof primitiveValue === 'boolean') {
      valueEl.textContent = primitiveValue ? 'ON' : 'OFF'
      fillEl.style.width = primitiveValue ? '100%' : '0%'
    } else {
      valueEl.textContent = primitiveValue === undefined ? '—' : String(primitiveValue)
      fillEl.style.width = '0%'
    }
  }

  // 宿主负责引用计数与重连重放；插件只声明自己关心的 tag。
  const off = ctx.tags.subscribe([w.tag], (changes) => {
    const change = changes.find((c) => c.tag === w.tag)
    if (change) render(change)
  })

  const current = ctx.tags.read(w.tag)
  if (current) render(current)

  ctx.log('widget mounted', w.id, w.tag)
  return () => off()
}

function mountPage(el, ctx, props) {
  const root = document.createElement('div')
  root.className = 'mcard-page'
  root.innerHTML = `
    <h3>${props.title || '插件示例'} · 页面贡献点</h3>
    <p>
      这个页面完全由插件自己的 ESM 代码渲染。插件通过 <code>ctx.tags.subscribe</code>
      读取实时值，由宿主按 tag 聚合去重；插件不直接建立 WebSocket。
    </p>
    <div class="mcard-page-grid">
      <div class="mcard-page-cell"><span>插件 id</span><strong>${ctx.plugin.id}</strong></div>
      <div class="mcard-page-cell"><span>版本</span><strong>v${ctx.plugin.version}</strong></div>
      <div class="mcard-page-cell"><span>页面 id</span><strong>${props.pageId || '—'}</strong></div>
      <div class="mcard-page-cell"><span>权限</span><strong>${(ctx.plugin.permissions || []).join(', ') || '无'}</strong></div>
    </div>
    <p>
      仪表盘组件 kind 为 <code>plugin:${ctx.plugin.id}:meter</code>；config 支持
      <code>{"min":0,"max":100,"decimals":1}</code>。删除插件后，已放置的组件会退化为占位符。
    </p>
    <button type="button" class="mcard-btn">测试 ctx.notify</button>`
  el.appendChild(root)

  const button = root.querySelector('.mcard-btn')
  const onClick = () => ctx.notify('示例插件：通知能力可用（manifest 已声明 notify）', 'success')
  button.addEventListener('click', onClick)

  return () => button.removeEventListener('click', onClick)
}
