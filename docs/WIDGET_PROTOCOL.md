# 组件协议 v2（Widget Protocol）

> 状态：P1–P4 已实施（2026-11-12）。目标是让「设备页面 = 小组件排列」成为宿主唯一模型。
> 本协议允许打破现有 `components[{kind,title,config}]` 结构；旧包通过兼容层继续可用。
>
> 落地进度：P1（点表写元数据 / `core:value`+`core:gauge` / `config` 透传 / 相对绑定）；
> P2（`core:switch`/`input`/`bits`、12 列 `rect` 网格、可见性订阅）；
> P3（插件与设备包 ESM 组件进设备页、`/packages` 静态路由、权限门面）；
> P4（包编辑器、`core:group` 声明式组合、`schemaVersion: 2` 页面、预设组合页）。
>
> **实现取舍**（与协议文本的差异，均已在代码中落地）：
>
> 1. **内存仍以 `components[]` 为渲染模型**：v2 页面（`schemaVersion: 2` + `widgets[]`）
>    在读取时翻译成 `components[]`（保留实例 `id`）。DTO/工作区也下发 `components[]`，
>    因此旧用户页面零迁移；导出包时保留作者写的原始 v2 JSON 形态。
> 2. **槽位 `many` 用数组**：`bind` 的值可以是单个声明或声明数组
>    （`"series": [{point:"uab"}, {point:"ia"}]`），Rust/TS 两端都接受。
> 3. **ESM 模块契约**：优先 `mountWidget(host)`；只有 `mount` 且形参 ≤1 时按 v2 host 调用；
>    `mount(el, ctx, props)`（形参 ≥2）走 v1 插件兼容路径（light DOM，样式全局）。
> 4. **静态路由只暴露声明目录**：`/packages/<id>/<version>/<path>` 只允许
>    `ui.widgets[].entry` 本身及其所在目录下的文件，其余一律 404。
> 5. **`$` 系统 tag 尚未进入槽位**：`$device/$channel/$host` 命名空间留在后续阶段，
>    当前 `PointMeta` 只解析设备点与位。

## 1. 目标与不变量

要达成的一句话愿景：

> 框架把每个数据点暴露成稳定的接口；小组件只通过接口取值/写值；
> 设备页面由小组件经布局组合而成；组件可由宿主内置、UI 插件或设备包提供。

三条不变量（协议的"宪法"）：

1. **一切可绑定的数据都有 tag**：设备点、位、以及设备/通道/宿主状态，统一寻址、统一订阅、统一质量戳。
2. **组件不接触寻址细节**：设备包里的组件只声明"我要绑哪个点"，设备实例 id 由宿主注入。
3. **页面是数据不是代码**：页面 = JSON 文档（布局 + 组件实例 + 绑定 + 配置），可直接序列化、版本化、用户可编辑。

## 2. 三层模型

```
┌──────────────────────────────────────────────┐
│ 页面层（Page）                                │
│  ui.pages[] = { layout, widgets[] }          │  包给出默认页 / 用户可覆盖
├──────────────────────────────────────────────┤
│ 组件层（Widget）                              │
│  manifest.ui.widgets[] + ESM 入口            │  内置 / 插件 / 设备包 同一协议
├──────────────────────────────────────────────┤
│ 数据层（Tag）                                 │
│  describe / subscribe / read / write          │  轮询 → TagHub → 订阅推送
└──────────────────────────────────────────────┘
```

页面只知道组件 `kind` 和绑定；组件只知道"槽位"；寻址与订阅全部由宿主负责。

## 3. 数据接口（Tag Protocol）

### 3.1 命名空间

| 形式 | 含义 | 例 |
|---|---|---|
| `<deviceId>.<pointId>` | 设备点值 | `pm1.uab` |
| `<deviceId>.<pointId>.b<N>` | 位展开 | `pm1.status.b2` |
| `$device.<deviceId>.<field>` | 设备系统数据（只读） | `$device.pm1.online` |
| `$channel.<channelId>.<field>` | 通道系统数据（只读） | `$channel.sim1.state` |
| `$host.<field>` | 宿主/链路状态（只读） | `$host.connected` |

- `$` 前缀保证与设备 id 不冲突（设备 id 不允许以 `$` 开头）。
- 设备点命名沿用现约定，不迁移。
- 系统数据**由宿主合成**（来自 `Snapshot` / `DeviceState` / `ChannelState` / WS 状态），gatewayd 协议不变——这是本期性价比最高的一刀。

`$device.*` 字段：`online`、`name`、`slave`、`channelId`、`package`、`points`（点总数）。
`$channel.*` 字段：`state`、`port`、`devices`、`online`。
`$host.*` 字段：`connected`、`hostVersion`、`uptimeMs`。

### 3.2 值模型

```ts
type Quality = 'good' | 'bad' | 'uncertain' | 'comm_lost'
type TagValue = { f64: number } | { i64: number } | { bool: boolean } | { str: string }
interface TagSample { tag: string; value: TagValue; quality: Quality; tsMs: number }
```

保持现有 `TagChange` 结构不变（插件与 UI 已依赖）。

### 3.3 描述接口（新增）

组件要"设置数据"就必须知道边界，宿主提供：

```ts
interface PointMeta {
  key: string                  // 包内相对键：pointId 或 `pointId.bN`
  tag: string                  // 解析后的完整 tag（只读，调试/展示用）
  name: string
  unit: string
  kind: 'number' | 'bool' | 'enum'
  access: 'r' | 'rw'
  dataType: string             // u16 / i32 / f32 / bcd16 ...
  scale?: number
  offset?: number
  min?: number                 // 新增：写值下限
  max?: number                 // 新增：写值上限
  step?: number                // 新增：步长
  enum?: { value: number; label: string }[]   // 新增：枚举点（整数工程量）
  bits?: { bit: number; name: string; on?: string; off?: string }[]
}
```

点表因此新增三个可选字段（`points.json`）：

```jsonc
{ "id": "mode", "type": "u16", "access": "rw",
  "min": 0, "max": 3, "step": 1,
  "enum": [ { "value": 0, "label": "自动" }, { "value": 1, "label": "手动" } ] }
```

校验规则：`min <= max`；`enum` 值必须落在 `[min,max]`；`enum` 仅允许整数类型；位区域点不支持 `enum`。

### 3.4 订阅 / 读 / 写

宿主对页面聚合后与 gatewayd 交互，组件看不到这一层：

- 订阅按 **tag 引用计数**去重（沿用现有机制），页面卸载自动退订。
- 只订阅**可见组件**用到的 tag（IntersectionObserver，提前 200px 预取）。
- gateway 侧已按 10~20ms 合并 `Changes`；UI 侧由 Vue 响应式批处理，
  组件只重渲染自身，不再叠加定时器。
- 写值走 `write`，宿主校验 `access === 'rw'`、类型与范围，越界直接拒绝并报组件。

### 3.5 质量与时间

每个值自带 `quality` 与 `tsMs`；组件必须自己决定如何呈现（灰值/角标/"--"）。
宿主不替组件做"坏值显示"策略，但提供 `theme.token('color.bad')` 等语义色。

## 4. 组件协议（Widget Protocol）

### 4.1 组件清单

写在提供方清单里（`manifest.json` 的 `ui.widgets` / 插件 manifest 的 `widgets`）：

```jsonc
{
  "kind": "com.acme.pm3000:trend",     // 全局唯一；推荐 `<包id>:<短名>`
  "title": "趋势图",
  "description": "多点时序曲线",
  "sdkVersion": 1,
  "entry": "widgets/trend.js",         // 内置组件不需要
  "size": { "w": 6, "h": 3, "minW": 3, "minH": 2 },
  "bindings": {
    "series": { "type": "point", "many": true, "required": true },
    "setpoint": { "type": "point", "write": true }
  },
  "config": {
    "windowSec": { "type": "number", "default": 60, "min": 10, "max": 3600 },
    "showLegend": { "type": "boolean", "default": true }
  },
  "permissions": ["tags:read"]
}
```

要点：

- `kind` 带命名空间前缀，杜绝内置/插件/包之间的撞名。
- `bindings` 声明槽位：`type: point | value | text`，`many` 允许多点，`write` 表示可写。
- `config` 是**配置 schema**（够用的子集即可），宿主据此生成编辑表单；运行时仍以 JSON 传入。
- `permissions`：`tags:read` / `tags:write` / `notify` / `net`（默认无）。

### 4.2 运行时宿主

```ts
interface WidgetHost {
  el: HTMLElement                            // 已挂载在 ShadowRoot 内
  mode: 'view' | 'edit'
  config: Readonly<Record<string, unknown>>

  slots: {
    sub(slot: string, cb: (samples: SlotSample[]) => void): () => void
    read(slot: string): SlotSample[] | undefined
    meta(slot: string): PointMeta | undefined
    write(slot: string, value: TagValue): Promise<void>
  }

  theme: { token(name: string): string; mode(): 'dark' | 'light' }
  notify(message: string, kind?: 'info' | 'success' | 'error'): void   // 需 notify 权限
  log(...args: unknown[]): void
}

interface SlotSample { meta: PointMeta; sample: TagSample }

interface WidgetModule {
  mount(host: WidgetHost): void | (() => void)   // 返回值 = 卸载函数
  update?(host: WidgetHost): void                // config/binding 变化
}
```

与现有插件 API 的差异（重要）：

- **槽位化**：组件不再拿 tag 字符串，而是 `slots.sub('value', cb)`；寻址、去重、系统数据全部由宿主完成。
- **多绑定**：`many: true` 时一个槽位返回数组，趋势图/多点表一次拿全。
- **`read()` 有语义**：返回最后一帧样本，组件可在 `mount` 时同步渲染，避免闪空。
- 现有 `PluginWidgetMount.widget.tag` 作为 v1 兼容路径保留一个版本周期。

### 4.3 绑定解析

- 包内页面：`{ "point": "uab" }` → 宿主解析为 `<当前设备id>.uab`。
- 包内位：`{ "point": "status", "bit": 2 }` → `<设备id>.status.b2`。
- 用户/工作区页面：`{ "tag": "pm1.uab" }` 绝对绑定（跨设备组态用）。
- **校验**：设备包清单里出现 `tag` 绝对绑定 → 安装时拒绝；用户页面允许。

### 4.4 生命周期

```
import(entry) → 缓存模块 → new ShadowRoot → mount(host)
   ↑ update(host)（config / 绑定变化，不重挂）
   ↓ 返回的 dispose() → 退订、清定时器、移除 shadow 内容
```

- 页面切换 = 逐个 dispose，订阅引用计数随之减少。
- 组件抛错：宿主捕获，替换为错误卡片（显示 kind + 错误信息），**不影响同页其他组件**。
- `mode === 'edit'` 时组件应禁用交互（沿用现有插件约定）。

### 4.5 样式隔离与主题

- 外部组件（插件 / 设备包）挂在 **ShadowRoot** 里，宿主注入：
  - `:host { all: initial; font: ... }` 基础样式；
  - 一组 CSS 变量（`--pm-color-bg`, `--pm-color-text`, `--pm-color-accent`, `--pm-color-bad`, ...），由 `theme.token()` 同源生成。
- 内置组件可不用 Shadow DOM（同源样式更快），但同样只使用 CSS 变量，便于整体换肤。
- 组件不得修改 `document.body` / 全局样式（评审项）。

### 4.6 错误与降级

| 情况 | 表现 |
|---|---|
| `kind` 未注册 | 占位卡：`未知组件 <kind>`，编辑模式提示安装来源 |
| 入口加载失败 | 占位卡 + 重试按钮 |
| 绑定缺失 | 编辑模式高亮"未绑定"，浏览模式渲染空态 |
| 运行时异常 | 该组件错误卡，其他组件不受影响 |
| 权限不足 | `write` 抛错，组件收到可读错误文案 |

## 5. 页面协议（Page Protocol）

### 5.1 页面结构

`ui.pages` 升级为 `schemaVersion: 2`：

```jsonc
{
  "schemaVersion": 2,
  "id": "overview",
  "title": "设备概览",
  "icon": "pulse",
  "layout": { "cols": 12, "rowHeight": 72, "gap": 12 },
  "widgets": [
    {
      "id": "w1",
      "kind": "core:value",
      "rect": { "x": 0, "y": 0, "w": 3, "h": 2 },
      "bind": { "value": { "point": "uab" } },
      "config": { "label": "AB 线电压", "precision": 1 }
    },
    {
      "id": "w2",
      "kind": "core:switch",
      "rect": { "x": 3, "y": 0, "w": 3, "h": 2 },
      "bind": { "value": { "point": "setpoint", "bit": 0 } },
      "config": { "onLabel": "合闸", "offLabel": "分闸" }
    }
  ]
}
```

- `rect` 用 12 列绝对格（与仪表盘一致），`y` 单值；宿主行高 72px、间距 12px。
- 缺 `rect` 时按数组顺序流式排布（迁移旧包用）。
- 窄屏（<900px）宿主强制 `w = 12` 重排，不写回数据。
- **P4 已实现**：`schemaVersion: 2` 页面读取时翻译为内存 `components[]`（保留实例 `id`）；
  安装/保存与 v1 共用同一套 rect/绑定校验。包导出仍保留作者写的 v2 `widgets[]` 形态。

### 5.2 页面归属

- **页面属于设备类型**（设备包），实例共用；绑定在渲染时解析为当前选中实例。
- 用户覆盖仍走 `workspaces.json`（同 id 覆盖页面、可增删排序），新增：可编辑 `widgets`。
- 每台设备**实例级**的自定义大屏继续由仪表盘组件承担（它是 `core:dashboard` 组件，内部有自己的 per-instance 布局文档），两者不冲突。

### 5.3 页面来源优先级

```
用户覆盖（workspaces.json） > 设备包 ui.pages > 宿主内置默认页
```

与现状一致，只是 `pages` 的内部结构换成 widgets。

## 6. 三种提供方

| 提供方 | 清单位置 | 代码 | 典型用途 |
|---|---|---|---|
| 内置 | 宿主编译进包 | 可选 Shadow DOM | 值/开关/仪表/文本/按钮/位表 |
| UI 插件 | 插件 `manifest.json` | ESM | 美化、专业展示、跨设备页面 |
| 设备包 | 设备包 `manifest.json` | ESM（可选） | 型号专属展示（如三相矢量图） |

- **设备包允许 ESM 组件**：包本来就是工程师安装的可信包，信任级别等同插件；协议、权限、生命周期完全一致。静态资源路由：`/packages/<id>/<version>/<entry 目录>`（仅暴露清单声明 entry 所在目录，阻止路径穿越）。**P3 已实现**。
- **声明式组合（无代码）作为子集支持**：`kind: "core:group"` 组件可以把若干子组件按 rect 组合，供不想写代码的包作者使用。**P4 已实现**（组内 12 列小网格，允许嵌套一层）。
- 模块缓存键：`(provider, kind, version)`；包升级自动失效（URL 带版本查询串）。

## 7. 效率规则（硬性）

1. **订阅聚合**：一个页面内同一 tag 只订阅一次，组件间共享；卸载按引用计数退。
2. **可见性订阅**：视口外组件不持有订阅（留 200px 预取），滚动进入时补订阅。
3. **批次合并**：沿用 gateway 侧 10~20ms 的 `Changes` 合并；Vue 响应式调度
   保证同批更新只触发一次组件重渲染，不额外加定时器。
4. **元数据缓存**：`PointMeta` 按设备类型缓存，包变更时整体失效；不按组件重复计算。
5. **组件隔离渲染**：每个组件独立挂载根，值变化不触发整页 vdom diff。
6. **按需加载**：ESM 入口按 kind 懒加载并缓存；页面级路由分包。
7. **编辑态静止**：`mode === 'edit'` 不订阅（或降频），避免编辑时刷屏。

验收指标（P2 结束时）：50 个组件 / 5 台设备的页面，空闲 CPU < 3%，切页 < 100ms，单 tag 10Hz 更新时页面无整页重排。

## 8. 安全与信任

- 设备包与插件都是**安装即信任**的可信代码；本期不做沙箱（iframe 化会牺牲主题与性能）。
- 权限在清单声明，宿主在 `host.slots.write` / `notify` 处强制校验。
- Shadow DOM 只解决样式污染，不是安全边界；文档中明确写出这一点。
- CSP 保持现状（`script-src 'self' ...`），静态资源仍走 `127.0.0.1:59120`。

## 9. 兼容与迁移（允许打破什么）

**打破**：

- `ui.pages[].components[{kind,title,config}]` → `widgets[{id,kind,rect,bind,config}]`。
- `ALLOWED_DEVICE_PAGE_COMPONENTS` 白名单删除 → 改为**组件注册表**校验（"kind 必须已注册"）。
- 插件 `props.widget.tag` → `host.slots`（旧字段保留一个版本）。

**保留**：

- tag 命名、`TagChange` 结构、订阅去重/重连重放、写值工程量换算。
- `ui.pages` 概念、workspace 覆盖、`dashboards.json`（实例级大屏）。
- 旧的四个 kind 注册为 **core 复合组件**，并按别名接受裸名：
  `overview → core:overview`、`dashboard → core:dashboard`、`tags → core:tags`、`settings → core:settings`。
  旧包 `schemaVersion: 1` 读取时把 `components` 翻译成"整行 widget"，安装时升级为 v2 内存结构，**不写回用户包**。

**新增字段**：点表 `min/max/step/enum`；清单 `ui.widgets[]`；页面 `layout`/`widgets`。

## 10. 落地阶段

| 阶段 | 内容 | 验收 |
|---|---|---|
| P1 ✅ | 点表写元数据（min/max/step/enum）+ `core:value`/`core:gauge` 进设备页 + `config` 透传 + 相对绑定 | 手写一个包，设备页出现小组件拼的页面（`examples/packages/pm3000` 的「运行监测」页即验收样例） |
| P2 ✅ | 内置组件表 + `core:switch`/`core:input`/`core:bits` + 页面渲染器（rect/流式）+ 可见性订阅 | 8 组件网格页（值/仪表/开关/输入/位表）在演示网关上完成写入回读 |
| P3 ✅ | 插件 widget 进设备页（同一 host）+ 设备包 ESM 组件 + 静态路由 + 权限 | 示例包自带一个趋势组件（pm3000「趋势」页） |
| P4 ✅ | 包编辑器（建包/填点表/预览/导出）+ overview/tags/settings 改写成预设组合 + v2 `widgets[]` | 全程不写 JSON 建出一个型号（编辑器演示包 → 设备页实时数据） |

P1 不动 gatewayd 采集侧（只加 DTO 字段与校验），是风险最低的切入点。

### P1 落地清单（2026-11-12）

- 点表：`PointDef.min/max/step/enum`；校验 `min <= max`、`step > 0`、
  `enum` 仅整数类型且值落在 `[min,max]`、位区域点禁止这些字段。
- 契约：`PointDto` 携带上述字段；`DevicePageComponentDto.bind` 携带
  `{ point, bit }`（相对）或 `{ tag }`（用户页面）。
- 安装校验：包内页面禁止绝对 `tag`；`point` 必须存在、`bit` 必须在 `bits` 中声明。
- 宿主：`core:value` / `core:gauge`（裸名兼容）渲染、`config` 透传、
  相对绑定解析为当前设备实例的 tag、按需订阅与退订。
- 用户页面编辑器：可直接编辑 `bind` JSON，支持绝对 tag 跨设备组态。
- 示例包：`pm3000` 新增「运行监测」页；`setpoint`/`mode` 带写值边界与枚举。

### P2 落地清单（2026-11-12）

- 页面模型：`components[].rect`（12 列，0 基）；整页全有或全无；
  安装/保存校验越界与重叠；窄屏自动单列。
- 内置组件：`core:value` / `gauge` / `switch` / `input` / `bits`（裸名兼容）。
- 写值：宿主校验 `access === 'rw'`、`min/max` 范围与类型；开关支持
  线圈点与整字点；位绑定用“读父点整字 → 改位 → 写回”，带 scale/offset 时拒绝。
- 输入：按 `PointMeta.step/enum/min/max` 生成数值或下拉；显式「写入」避免误触。
- 位表：一个父点展开全部 `bits`，逐位显示文案与质量。
- 效率：IntersectionObserver（提前 200px）门控订阅；同 tag 引用计数去重；
  卸载即退订。
- 页面编辑器：网格/流式一键切换、x/y/w/h 编辑、保存前校验重叠与越界。
- 示例页：8 组件 4×2 网格（值/仪表/位值/数值输入/枚举输入/开关/位表）。

### P3 落地清单（2026-11-12）

- **组件清单**：设备包 `ui.widgets[]`（id/title/entry/size/bindings/config/permissions）；
  全局 kind 为 `<包id>:<短名>`，页面里可写短名（读取时归一化）。安装校验 id/entry/权限/尺寸，
  并要求 entry 文件真实存在。
- **静态服务**：`GET /packages/<id>/<version>/<path>`，只暴露声明 entry 所在目录；
  版本必须等于已安装版本；CORS + `no-cache`。
- **运行时宿主**（`plugins/widgetHost.ts`）：
  - v2：ShadowRoot + `:host` 基础样式 + `--pm-color-*` 主题变量；
    `slots.sub/read/meta/write`、`theme.token`、`notify`、`log`；
  - 可见性门控（200px 预取）与 tag 引用计数；写位走“读父点整字 → 改位 → 写回”；
  - 权限门面：`tags:read`（兼容 `tags:subscribe`）/ `tags:write` / `notify`；
  - v1 插件兼容：`mount(el, ctx, props)` 在 light DOM 挂载，`ctx.tags` 被限制在该组件
    bind 声明的 tag 集合内。
- **设备页编辑器**：组件下拉合并核心组件、当前包的 ESM 组件与已加载插件组件；
  `bind` 支持单声明或数组（多点槽位）。
- **示例**：`pm3000/widgets/trend.js`（多点趋势，无依赖 ESM）；「趋势」页 v2 `widgets[]`。
  演示网关会把 `widgets/` 一并种入临时安装目录。

### P4 落地清单（2026-11-12）

- **v2 页面**：`schemaVersion: 2` + `layout` + `widgets[{id,kind,rect,bind,config}]`；
  读取时翻译为 `components[]`；安装/保存校验实例 id 重复、混用 v1/v2 写法、组合嵌套。
- **包编辑器**（设备包页「新建设备包」）：基本信息 / 点表 / 页面 / 预览与导出四步：
  - 点表：轮询组、点位、scale/offset/unit、min/max/step、bits（位号/文案）、enum；
  - 页面：`core:value/gauge/switch/input/bits/group` 组合、结构化绑定点/位、rect；
  - 「按点表生成预设页面」：概览（数值卡+状态位）/ 参数（输入）/ 状态位 / 实时点表 / 设备设置；
  - 校验调用 gatewayd `validatePackage`（与安装同规则），errors 非空禁止安装/导出；
  - 安装走 `installDraftPackage`（临时目录 → 正式安装流）；导出走 `exportPackage`
    （只写 manifest.json + points.json，保留 v2 原始形态）；草稿自动存 localStorage。
- **`core:group`**：`config.items[]` 子组件声明（kind/title/rect/bind/config），
  组内 12 列小网格；安装校验子组件 kind/rect/绑定，禁止直接嵌套自身。
- **预设组合兜底**：设备包未声明页面且有点表时，宿主按点表生成组合页
  （概览/参数/状态位 + `core:tags`/`core:settings` 复合组件）；无点表仍回落到原四页。
- **复合组件别名**：`core:overview` / `core:dashboard` / `core:tags` / `core:settings`
  与裸名等价；`core:group` 为新增声明式组件。
