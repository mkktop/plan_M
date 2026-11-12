# PM3000 示例设备包

演示设备包的各项能力：

- `u16` + scale（电压/电流）
- `i32`/`u32` + `cdab` 字交换字节序（功率/电度 —— 国产电表最常见排列）
- 状态字 `bits` 位展开（`status.b0` 运行 / `status.b2` 报警）
- `rw` 可写点（setpoint 带 min/max/step，mode 带枚举）
- 双轮询组：`fast` 1s / `slow` 60s（电度慢采，省 485 带宽）
- 包内 ESM 组件：`widgets/trend.js` 多点实时趋势（`ui.widgets[]` 声明、`/packages` 静态路由）
- 页面两种写法并存：`overview` 用 v1 `components[]`，`monitor`/`trend` 用 v2
  `schemaVersion: 2 + widgets[]`（`layout` 12 列、行高 72px）

## 用法

```bash
# 校验
gatewayd pkg validate examples/packages/pm3000

# 无硬件轮询（虚拟从站预置数据）
gatewayd poll --package examples/packages/pm3000 --com SIM --once

# 真硬件轮询
gatewayd poll --package examples/packages/pm3000 --com COM3 --slave 1

# 安装到正式包目录
gatewayd pkg install examples/packages/pm3000
```

地址记法：`40101` = 保持寄存器第 101 个（0 基偏移 100），与设备手册口径一致。
