# 网关 WebSocket API v2

gatewayd 默认监听 `127.0.0.1:59120`。客户端连接 `ws://127.0.0.1:59120/ws`，以 WebSocket 文本消息发送 JSON 对象。每条消息有 camelCase 的 `type` 判别字段，字段名也采用 camelCase。当前网关在 `welcome.apiVersion` 中报告 **2**，`hostVersion` 是应用版本。 [消息和 DTO 定义](../crates/wire/src/lib.rs) · [服务端处理器](../apps/gatewayd/src/serve.rs)

## 握手与最小联调

连接打开后先发送 `{"type":"hello","token":"<gateway.token 的内容>"}`，等收到 `welcome` 再发业务命令。令牌文件位于 gatewayd 的数据目录，Windows 默认是 `%APPDATA%\gatewayd\gateway.token`。`--data-dir` 可指定其他目录。令牌错误或以其他业务请求开始握手时，网关发送 `error` 后关闭连接。

`welcome` 返回 `hostVersion`、`apiVersion`、`transports[]` 和 `drivers[]`。驱动条目有 `id`、`name`、`transports[]`、`pointAddressFormat`、`scanBounds` 和 `scanRequiresPackage`。根据能力列表选择传输和驱动，并以 `scanBounds` 设置扫描范围。

从仓库根目录启动独立的双电表模拟服务。示例用 59121，端口已被占用时可同时调整服务参数和客户端地址。

```powershell
cargo run -p hmi-gatewayd -- serve --sim --port 59121 --data-dir "$env:TEMP/planm-ws-demo"
```

将下面代码保存为 `ws-demo.mjs`。需要 Node.js 22 或更新版本。程序从令牌文件读取凭据，完成握手和快照请求，再订阅模拟电表 `pm1.uab`，收到一条 `good` 读数后退出。

```javascript
import { readFileSync } from 'node:fs'

const [, , tokenFile, endpoint = 'ws://127.0.0.1:59121/ws'] = process.argv
if (!tokenFile) throw new Error('请提供 gateway.token 的路径')

const token = readFileSync(tokenFile, 'utf8').trim()
const socket = new WebSocket(endpoint)
const send = (message) => socket.send(JSON.stringify(message))
const tag = 'pm1.uab'
let complete = false

const timer = setTimeout(() => {
  console.error('10 秒内没有收到有效读数')
  socket.close()
}, 10_000)

socket.addEventListener('open', () => send({ type: 'hello', token }))
socket.addEventListener('message', ({ data }) => {
  const message = JSON.parse(data)
  switch (message.type) {
    case 'welcome':
      if (message.apiVersion === 2) send({ type: 'getSnapshot' })
      else {
        console.error('不支持的 API 版本', message.apiVersion)
        socket.close()
      }
      break
    case 'snapshot':
      console.log('设备', message.devices.map(({ id }) => id))
      send({ type: 'subscribe', tags: [tag] })
      break
    case 'changes': {
      const sample = message.items.find((item) => item.tag === tag && item.quality === 'good')
      if (sample) {
        complete = true
        console.log(sample)
        socket.close()
      }
      break
    }
    case 'error':
      console.error(message.message)
      socket.close()
      break
  }
})
socket.addEventListener('error', () => {
  if (!complete) console.error('WebSocket 连接失败')
})
socket.addEventListener('close', () => {
  clearTimeout(timer)
  if (!complete) process.exitCode = 1
})
```

```powershell
node ws-demo.mjs "$env:TEMP/planm-ws-demo/gateway.token"
```

## 快照、订阅与重连

| 客户端请求 | 成功响应 | 含义 |
|---|---|---|
| `hello{token}` | `welcome{hostVersion,apiVersion,transports,drivers}` | 认证并返回宿主能力 |
| `getSnapshot` | `snapshot{channels,devices,packages,tags}` | 通道、设备、当前启用的设备包及已缓存读数 |
| `subscribe{tags}` | `changes{items}` | 把 tag 加入本连接的订阅集合，然后返回该连接订阅集合中已缓存的读数。之后推送增量批次 |
| `unsubscribe{tags}` | `commandOk{kind:"unsubscribe"}` | 从本连接的订阅集合移除指定 tag |
| `writeTag{tag,value}` | `commandOk{kind:"writeTag"}` | 驱动完成写入事务。通过后续订阅读数确认读回 |

一个 `changes` 批次可包含多个点，也可能在合并窗口内包含同一 tag 的多次变化。没有缓存的 tag 不出现在订阅首包中。慢客户端可能丢失批次，需要时请求新快照。订阅随连接关闭释放。重连后重新握手、拉取快照并重放订阅。

服务端还会主动推送 `channelState{channel,state,detail}` 和 `deviceState{device,online}`。通道状态为 `running`、`recovering`、`faulted`、`stopped`。通道状态和设备在线状态是独立字段，设备状态按实例更新。

服务端返回的业务或解析错误形如 `{"type":"error","message":"…"}`。v2 没有请求 ID，`changes` 既可作为订阅首包也可主动推送。客户端宜串行发送需要配对的请求，按预期 `type` 和可用字段判断应答，同时独立处理状态和点值推送。写入等待超时后，可先确认实际读数再决定是否重试。

## 点值与质量

tag 使用 `<设备实例 ID>.<点位 ID>`，状态字位展开为 `<设备实例 ID>.<点位 ID>.b<位号>`。读数的 `value` 是带类型的对象，支持 `{"f64":220.1}`、`{"i64":300}`、`{"bool":true}`、`{"str":"text"}`。JavaScript 客户端处理可能超出安全整数范围的 `i64` 时，应使用能保留整数精度的 JSON 解析器。

```json
{
  "type": "changes",
  "items": [
    { "tag": "pm1.uab", "value": { "f64": 220.1 }, "quality": "good", "tsMs": 1794988800000 }
  ]
}
```

| 质量 | 含义 |
|---|---|
| `good` | 读数有效 |
| `bad` | 数值内容异常或不可用 |
| `uncertain` | 质量存疑 |
| `comm_lost` | 设备通信中断，值可能来自缓存或占位 |

`tsMs` 是网关生成的 Unix 毫秒时间。对 `comm_lost`，它表示失联状态更新的时间。若需要展示末次有效采样时间，应单独保留之前的有效样本。WebSocket 客户端连不上网关时，应把本地缓存的在线状态和读数标为待确认。

写值使用工程量，目标必须是 `rw` 主点。Modbus 线圈用 `bool`，位展开子 tag 只读。网关检查适用的范围、步长和枚举，驱动负责反算并编码。以下命令可用于模拟电表。

```json
{"type":"writeTag","tag":"pm1.setpoint","value":{"i64":300}}
```

## 通道和设备实例

| 客户端请求 | 成功响应 | 含义 |
|---|---|---|
| `getTransportTargets{transport}` | `transportTargets{transport,targets}` | 枚举当前可发现的目标，例如串口 |
| `upsertChannel{config}` | `snapshot` | 新建或更新通道，影响该通道的连接及轮询 |
| `removeChannel{channel}` | `snapshot` | 删除通道及其下设备 |
| `addDevice{config}` | `snapshot` | 新建或更新设备实例 |
| `removeDevice{device}` | `snapshot` | 以设备实例 ID 移除设备 |

通道的 `timing` 是嵌套对象，传输参数与 `transport` 同级。以下是 TCP 通道格式，应用到现场时换成实际地址和端口。

```json
{
  "type": "upsertChannel",
  "config": {
    "id": "tcp1",
    "name": "测试通道",
    "transport": "tcp",
    "host": "127.0.0.1",
    "port": 15020,
    "connectTimeoutMs": 3000,
    "timing": { "respTimeoutMs": 200, "retries": 2, "probeIntervalMs": 5000 }
  }
}
```

串口通道使用 `port`（如 `"COM3"`）、`hwId`、`baudRate`、`dataBits`、`parity`、`stopBits`、`interFrameGapMs`。SIM 通道可带 `seed`。设备配置包含 `id`、`channelId`、`package`、`address`，可选 `name` 和 `overrides`。实例地址按驱动解释，当前 Modbus 和固定帧都要求 JSON 数字。点级覆盖使用 `points.<pointId>.scale`、`points.<pointId>.offset` 或 `points.<pointId>.pollGroup`。

```json
{
  "type": "addDevice",
  "config": { "id": "meter1", "channelId": "tcp1", "package": "com.acme.pm3000", "address": 1 }
}
```

`snapshot.channels` 在通道配置字段外带 `state`，`snapshot.devices` 在设备配置字段外带 `online`，两者都可能带 `configError`。`snapshot.packages[].points[].bits` 在 API 中是数组，元素是 `{bit,name,on,off}`。设备包的源文件 `points.json` 用位号作对象键。 [配置类型和默认值](../crates/domain/src/config.rs) · [设备包格式](DEVICE_INTEGRATION.md)

### 扫描设备地址

`scanChannel` 必填 `channel`，可选 `driver`、`package` 和 `range`。范围包含两端，缺省扫描 1–32。固定帧扫描应显式传已经安装的 `package`，也可同时传一致的 `driver: "frame"`。Modbus 透传扫描也应选用配置了相应 `framing` 的包。需要时从 `welcome.drivers` 取驱动范围。

```json
{
  "type": "scanChannel",
  "channel": "tcp1",
  "driver": "frame",
  "package": "com.example.fixed-frame-meter",
  "range": { "from": 6, "to": 7 }
}
```

扫描结束返回 `{"type":"scanDone","channel":"tcp1","found":[7]}`，找不到时 `found` 为空数组。当前服务只在结束时返回结果，`scanProgress` 是消息定义中的预留类型。扫描与该通道的轮询共享链路，扫描完成前同一 WebSocket 上的后续请求会排队。可按地址数和超时设置调整客户端等待时间。

## 设备包、布局与插件

| 客户端请求 | 成功响应 | 含义 |
|---|---|---|
| `getPackages` | `packages{packages}` | 列出当前启用的设备包 |
| `getPackageVersions{id}` | `packageVersions{id,versions}` | 查询指定设备包的已安装版本 |
| `activatePackageVersion{id,version}` | `snapshot` | 按 semver 切换启用版本 |
| `importPackage{path}` | `snapshot` | 从 gatewayd 本机目录或 ZIP 导入 |
| `removePackage{id}` | `snapshot` | 删除该 ID 的全部版本。在用时返回错误 |
| `validatePackage{manifest,points}` | `packageValidation{errors,warnings}` | 校验设备包草稿，`manifest`、`points` 为 JSON 对象。无法解析时也可能返回 `error` |
| `installDraftPackage{manifest,points}` | `snapshot` | 校验并安装草稿 |
| `exportPackage{manifest,points,dir}` | `commandOk{kind:"exportPackage"}` | 把草稿文件导出到 gatewayd 本机目录 |
| `getDashboards`、`saveDashboard{dashboard}`、`removeDashboard{id}` | `dashboards{dashboards}` | 读取或更新仪表盘布局，返回完整列表 |
| `getWorkspaces`、`saveWorkspace{workspace}`、`removeWorkspace{deviceType}` | `workspaces{workspaces}` | 读取、保存或移除工作区覆盖，返回完整列表 |
| `getUiPlugins`、`importUiPlugin{path}`、`removeUiPlugin{id}` | `uiPlugins{plugins}` | 读取、导入或移除 UI 插件。导入路径在 gatewayd 本机 |

仪表盘对象使用 `{id,name,cols,scopeKind,scopeId,widgets}`。设备仪表盘使用 `scopeKind: "device"` 和实例 ID，系统仪表盘省略这两个字段。网格默认 12 列。`widgets` 元素使用 `{id,kind,title,tag,unit,config,layout}`，其中 `layout:{x,y,w,h}` 是 0 基网格坐标，保存时校验边界和重叠。组件根据 `kind` 解释 `config`，例如内置 `value` 使用 `decimals`。

工作区对象使用 `{deviceType,pages}`。`deviceType` 是历史字段名，可以是设备包 ID、`device:<deviceId>`、`system` 或 `system-plugin:<pluginId>:<pageId>`。插件覆盖包含一页，页 ID 固定为 `page`。API 页面使用 `{id,title,icon,components}`，组件可带 `id`、`kind`、`title`、`bind`、`rect`、`config`。设备包 v2 清单的 `widgets[]` 加载时归一化成 `components[]`。槽位绑定和布局规则见组件协议。 [组件与页面协议](WIDGET_PROTOCOL.md)

`uiPlugins.plugins[]` 提供 `id`、`name`、`version`、`description`、`author`、`sdkVersion`、`permissions`、`entry`、`widgets`、`pages` 和 `devicePanels`。客户端可用 `id`、`entry` 拼接 HTTP 资源地址。 [插件 SDK 类型](../apps/desktop/ui-plugin-sdk/index.d.ts)

## HTTP 端点与验证

同一端口提供 `GET /health`（`{status,name,version}`）、`GET /plugins`、`GET /plugins/<id>/<path>` 和 `GET /packages/<id>/<version>/<path>`。后两个端点用于加载已安装的 ESM 组件资源。gatewayd 当前绑定回环地址，连接使用本地明文 HTTP／WS。

自动化协议联调从仓库根目录执行 `cargo test -p hmi-gatewayd --test ws_smoke`。固定帧驱动的扫描、读写和回读用 `cargo test -p hmi-gatewayd --test frame_package` 验证。 [WS 测试](../apps/gatewayd/tests/ws_smoke.rs) · [固定帧测试](../apps/gatewayd/tests/frame_package.rs)
