//! poll 子命令：直接对一条串口/TCP/SIM 链路轮询一个设备包并打印。
//! 无硬件调试的第一工具。

use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use hmi_domain::config::{
    ChannelConfig, ChannelTiming, DeviceAddress, Parity, SerialParams, SimParams, TcpParams,
    TransportConfig,
};
use hmi_domain::driver::{CompileInput, Driver};
use hmi_domain::link::{TransportFactory, TransportKind};
use hmi_packaging::load_package_dir;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum TransportArg {
    Serial,
    Tcp,
    Sim,
}

impl TransportArg {
    pub fn kind(self) -> TransportKind {
        match self {
            TransportArg::Serial => TransportKind::Serial,
            TransportArg::Tcp => TransportKind::Tcp,
            TransportArg::Sim => TransportKind::Sim,
        }
    }
}

pub struct PollArgs {
    pub package: std::path::PathBuf,
    pub transport: TransportArg,
    pub com: Option<String>,
    pub host: Option<String>,
    pub tcp_port: u16,
    pub address: u32,
    pub baud: u32,
    pub interval_ms: u64,
    pub once: bool,
}

pub async fn run(args: PollArgs) -> Result<()> {
    let drivers = crate::drivers();
    let pkg = load_package_dir(&args.package, &drivers)
        .with_context(|| format!("加载设备包 {}", args.package.display()))?;
    println!(
        "设备包: {} v{} （驱动 {}，{} 点）",
        pkg.manifest.name,
        pkg.manifest.version,
        pkg.manifest.protocol.driver,
        pkg.points.points.len()
    );

    let driver = drivers
        .iter()
        .find(|d| d.id() == pkg.manifest.protocol.driver)
        .cloned()
        .with_context(|| format!("驱动 `{}` 未注册", pkg.manifest.protocol.driver))?;

    let channel = build_channel(
        "cli",
        args.transport,
        args.com.as_deref(),
        args.host.as_deref(),
        args.tcp_port,
        args.baud,
        300,
    )?;
    println!(
        "链路: {} {}",
        args.transport.kind(),
        transport_summary(&channel)
    );

    let factory = factory_for(args.transport);
    let link = factory
        .open(&channel)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let overrides = Default::default();
    let plan = driver.compile(&CompileInput {
        device_id: "dev",
        overrides: &overrides,
        points: &pkg.points,
        protocol: &pkg.manifest.protocol,
    })?;
    let timeout = Duration::from_millis(300);
    let address = DeviceAddress::Number(args.address);

    println!("轮询周期: {}ms，Ctrl-C 退出\n", args.interval_ms);
    let mut cycle = 0u64;
    loop {
        cycle += 1;
        let t0 = std::time::Instant::now();
        println!("── 第 {cycle} 轮 ──");
        for g in &plan.groups {
            match driver.poll(link.as_ref(), &address, g, timeout).await {
                Ok(mut samples) => {
                    samples.sort_by_key(|s| s.tag.to_string());
                    for s in samples {
                        println!(
                            "  {:<24} {:>14}  [{:<9}] {}",
                            s.tag,
                            fmt_value(&s.value),
                            s.quality,
                            g.name
                        );
                    }
                }
                Err(e) => println!("  [{}] 组 `{}` 读取失败: {e}", g.name, g.name),
            }
        }
        if args.once {
            println!("\n完成（--once）");
            return Ok(());
        }
        let elapsed = t0.elapsed();
        if elapsed < Duration::from_millis(args.interval_ms) {
            tokio::time::sleep(Duration::from_millis(args.interval_ms) - elapsed).await;
        }
    }
}

pub fn build_channel(
    id: &str,
    transport: TransportArg,
    com: Option<&str>,
    host: Option<&str>,
    tcp_port: u16,
    baud: u32,
    timeout_ms: u32,
) -> Result<ChannelConfig> {
    let transport = match transport {
        TransportArg::Serial => {
            let com = com.context("--com 必填（串口名，如 COM3）")?;
            TransportConfig::Serial(SerialParams {
                port: com.to_string(),
                hw_id: None,
                baud_rate: baud,
                data_bits: 8,
                parity: Parity::None,
                stop_bits: 1,
                inter_frame_gap_ms: 5,
            })
        }
        TransportArg::Tcp => {
            let host = host.context("--host 必填（TCP 网关地址）")?;
            TransportConfig::Tcp(TcpParams {
                host: host.to_string(),
                port: tcp_port,
                connect_timeout_ms: 3000,
            })
        }
        TransportArg::Sim => TransportConfig::Sim(SimParams { seed: None }),
    };
    Ok(ChannelConfig {
        id: id.into(),
        name: String::new(),
        timing: ChannelTiming {
            resp_timeout_ms: timeout_ms,
            retries: 1,
            probe_interval_ms: 5000,
        },
        transport,
    })
}

pub fn factory_for(transport: TransportArg) -> Arc<dyn TransportFactory> {
    match transport {
        TransportArg::Serial => Arc::new(hmi_transport_serial::SerialFactory),
        TransportArg::Tcp => Arc::new(hmi_transport_tcp::TcpFactory),
        TransportArg::Sim => crate::simnet::sim_factory(),
    }
}

pub fn transport_summary(channel: &ChannelConfig) -> String {
    match &channel.transport {
        TransportConfig::Serial(p) => format!("{} @ {} 8N1", p.port, p.baud_rate),
        TransportConfig::Tcp(p) => format!("{}:{}", p.host, p.port),
        TransportConfig::Sim(_) => "SIM（虚拟，预置演示数据）".into(),
        TransportConfig::Ble(p) => format!("BLE {}", p.address),
    }
}

/// 数值显示：收敛浮点尾差（如 2201×0.1 = 220.10000000000002），最多 6 位小数。
fn fmt_value(v: &hmi_domain::value::Value) -> String {
    match v {
        hmi_domain::value::Value::F64(f) => {
            let s = format!("{f:.6}");
            let s = s.trim_end_matches('0').trim_end_matches('.');
            if s.is_empty() || s == "-" {
                "0".to_string()
            } else {
                s.to_string()
            }
        }
        other => other.to_string(),
    }
}

/// 供 scan/poll 输出：判断路径是否存在的友好报错。
pub fn ensure_package_dir(p: &std::path::Path) -> Result<()> {
    if !p.is_dir() {
        bail!("设备包目录不存在: {}", p.display());
    }
    Ok(())
}

/// 编译期检查驱动注册表类型（避免 import 被误删）。
#[allow(dead_code)]
fn _driver_trait_object(d: Arc<dyn Driver>) -> Arc<dyn Driver> {
    d
}
