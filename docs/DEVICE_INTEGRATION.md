# 设备接入与固定帧配置

一个采集通道描述串口、TCP 或虚拟总线连接。设备包描述型号的协议、点位和换算规则。设备实例将设备包和通道上的地址关联起来，由实例 ID 生成实时 tag，例如 `pm1.uab`。

| 设备手册给出的通信方式 | 设备包驱动 | 通道与包参数 |
|---|---|---|
| Modbus RTU | `modbus` | 串口用 `framing: "auto"` 或 `"rtu"`。通过透明串口服务器走 TCP 时用 `"rtu"` |
| Modbus TCP | `modbus` | TCP 用 `framing: "auto"` 或 `"tcp"`，发送 MBAP 封装 |
| 帧长度和应答前缀固定的厂商协议 | `frame` | 串口或 TCP，按设备手册填写探测、读取和写入帧 |
| 无硬件演示 | `modbus` | 虚拟总线（SIM），使用预置电表 |

如果设备依赖变长帧、动态 CRC 或校验和、握手、主动上报，或需要解析固定应答前缀之外的校验字段，按扩展指南实现专用驱动。现有宿主注册串口、TCP 和 SIM 三种传输。 [协议扩展指南](PROTOCOL_EXTENSION.md)

## 从空项目接入设备

1. 在「设备」页点「创建通道」。串口按设备手册填写 COM 口、波特率、数据位、校验位和停止位。TCP 填设备或网关的地址和端口。应答超时、重试次数和离线探测周期在通道设置中配置。
2. 导入与该通道兼容的设备包。目录或 ZIP 包含 `manifest.json` 与 `points.json`，采用相应型号的包。桌面端可选文件或拖放，浏览器调试使用 **gatewayd 所在机器**的绝对路径。没有现成包时，可以在「设备包 → 新建设备包」填写协议和点位。 [清单 Schema](../crates/packaging/schema/manifest.schema.json) · [点表 Schema](../crates/packaging/schema/points.schema.json)
3. 已知地址时直接填写。否则在通道组头点「扫描」，选协议或设备包，输入起止地址。Modbus 地址为 1–247，固定帧为 0–255。固定帧扫描应选择已安装的对应设备包。扫描发现应答地址后可以回填，仍需根据设备手册或铭牌确认型号。
4. 添加设备，检查首台设备的「采集验证」是否收到有效读数。再到工作区「实时点表」核对单位、数值、采样和质量。验证写值时选择已确认可修改的 `rw` 点，在测试设备上按工程量写入，并等待后续轮询读回。

「诊断 → 跨设备数据浏览」可在设备间切换。网关连接中断时，桌面端把设备状态标为待确认，点表保留缓存读数和末次采样时间，写值入口暂停。设备通信中断但网关仍连接时，点表可收到设备侧失联质量。

## Modbus 点表与快速验证

`40101` 代表保持寄存器的 0 基偏移 100，可写成 `HR:100`。输入寄存器用 `30001` 或 `IR:0`，线圈用 `00001` 或 `C:0`，离散输入用 `10001` 或 `DR:0`。先核实厂商手册使用的是工程编号还是 PDU 偏移，再换算为点表写法。

多字值应按手册明确 `byteOrder`，支持 `abcd`、`badc`、`cdab`、`dcba`。读取后的工程量为 `原始值 × scale + offset`，默认 `scale=1`、`offset=0`。点位可通过 `pollGroup` 归入快、慢轮询组，通过 `bits` 展开为只读子点。可写点使用 `access: "rw"`，其 `min`、`max`、`step`、`enum` 使用工程量口径。 [PM3000 点表](../examples/packages/pm3000/points.json)

从仓库根目录校验示例包，并在无硬件条件下轮询一轮。

```powershell
cargo run -p hmi-gatewayd -- pkg validate examples/packages/pm3000
cargo run -p hmi-gatewayd -- poll --package examples/packages/pm3000 --transport sim --once
```

要用桌面界面走一遍，先在单独的数据目录启动演示服务，再启动桌面端或浏览器前端。若另一个网关已占用默认端口，可另选端口并让浏览器使用对应的 `ws` 参数。 [桌面开发说明](../apps/desktop/README.md)

```powershell
cargo run -p hmi-gatewayd -- serve --sim --data-dir "$env:TEMP/planm-device-demo"
```

## 固定帧设备包字段

以仓库的 [固定帧演示包](../examples/packages/fixed-frame-meter) 为起点。清单使用 `schemaVersion: 2`，协议块包含 `driver: "frame"`、`transports: ["serial", "tcp"]` 和 `params.probe`。这里的 `params.probe` 是设备扫描和离线探测使用的帧，包内各点的读写帧可以不同。

| 所在位置 | 字段 | 含义和约束 |
|---|---|---|
| `protocol.params` | `probe` | 必填的探测帧对象，使用下述帧字段 |
| `probe`、`address.read`、`address.write.frame` | `request` | 发送字节串，使用空白分隔的两位十六进制字节，可多次使用 `{unit}` |
| 同上 | `responseBytes` | 读取的应答总字节数，包含前缀 |
| 同上 | `responsePrefix` | 从应答第一个字节开始校验的前缀，可使用 `{unit}` |
| 点位 `address` | `read`、`valueOffset` | 读帧对象，以及从应答第一个字节起算的 0 基数值偏移。数值字段必须位于前缀之后且完整落在应答内 |
| 点位 `address` | `write.frame`、`write.valueOffset` | 写帧对象，以及从请求第一个字节起算的 0 基数值偏移。`rw` 点必填。数值覆盖区的原始请求字节均须为 `00` |

`{unit}` 代入设备实例的单字节数值地址（0–255）。请求和应答前缀各至少 1 字节，请求与应答总长均不超过 512 字节。驱动按 `responseBytes` 收满应答并核对前缀。

固定帧使用点表的 `type` 决定占用字节数。16 位类型占 2 字节，32 位类型占 4 字节，64 位类型占 8 字节。字节序由点的 `byteOrder` 决定，缺省 `abcd`。读取后应用 `scale` 和 `offset`，写入时按 `(工程量 - offset) / scale` 反算，再编码进预留字节。驱动按组调度，但每个固定帧点各发一条读请求，点数较多时应根据设备响应时间安排轮询组。 [固定帧驱动](../crates/driver-frame/src/lib.rs)

### 示例电表的可写点

以下是 `points.json` 中的一条点位定义。`protocol.params.probe` 使用与本点 `address.read` 相同的帧。

```json
{
  "id": "setpoint",
  "name": "设定值",
  "type": "u16",
  "access": "rw",
  "unit": "W",
  "scale": 0.5,
  "min": 0,
  "max": 1000,
  "step": 0.5,
  "address": {
    "read": {
      "request": "{unit} 10",
      "responseBytes": 4,
      "responsePrefix": "{unit} 90"
    },
    "valueOffset": 2,
    "write": {
      "frame": {
        "request": "{unit} 11 00 00",
        "responseBytes": 2,
        "responsePrefix": "{unit} 91"
      },
      "valueOffset": 2
    }
  }
}
```

设备地址为十进制 7 时，发送 `07 10`。应答 `07 90 00 FA` 解出原始值 250，对应 **125 W**。写入 **150.5 W** 时，原始值为 301（`01 2D`），发送 `07 11 01 2D` 并等待 `07 91` 应答。下一轮读取应答 `07 90 01 2D` 才能确认读回目标值。

### 从校验到验收

```powershell
cargo run -p hmi-gatewayd -- pkg validate examples/packages/fixed-frame-meter
cargo test -p hmi-gatewayd --test frame_package
```

上述测试自动启动临时 TCP 模拟设备，导入设备包，扫描地址 6–7 并找到 7，读取 125 W，写入 150.5 W 后校验回读。测试端口和工程目录临时分配。 [固定帧集成测试](../apps/gatewayd/tests/frame_package.rs)

现场调试时先导入待测设备包，再在桌面「设备 → 扫描」选该包，或在 `scanChannel` 指定 `package`。CLI 的 `gatewayd scan` 使用 Modbus 探测。已知地址时，可用 `gatewayd poll --package <设备包目录> --transport tcp --host <设备地址> --tcp-port <端口> --address 7 --once` 直接读一轮。 [WebSocket 扫描接口](WEBSOCKET_API.md#扫描设备地址)

| 现象 | 优先检查 |
|---|---|
| 扫描不到设备或读不到数据 | 通道与设备包是否兼容，地址范围、串口参数或 TCP 端口是否正确。固定帧还要核对 `probe.request`、应答长度与前缀 |
| 扫描成功但点位读数异常 | 区分探测帧和点位读帧。对照抓到的应答核对长度、前缀、数值偏移、类型、字节序和换算 |
| 写入被拒绝 | 核对点位的读写属性、工程量范围、步长、枚举和写帧占位 |
| 写入应答成功但回读不一致 | 核对设备是否接受并应用目标值，再检查读写帧指向的是不是同一参数 |
