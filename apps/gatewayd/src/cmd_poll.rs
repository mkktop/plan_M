//! poll 子命令：不开 UI、不开服务，直接对一条串口/虚拟总线轮询一个设备包并打印。
//! 无硬件调试的第一工具。

use anyhow::{bail, Context, Result};
use hmi_domain::config::{Parity, SerialChannelConfig};
use hmi_packaging::load_package_dir;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

pub struct PollArgs {
    pub package: std::path::PathBuf,
    pub com: String,
    pub slave: u8,
    pub baud: u32,
    pub interval_ms: u64,
    pub once: bool,
}

pub async fn run(args: PollArgs) -> Result<()> {
    let pkg = load_package_dir(&args.package)
        .with_context(|| format!("加载设备包 {}", args.package.display()))?;
    println!(
        "设备包: {} v{} （驱动 {}，{} 点）",
        pkg.manifest.name,
        pkg.manifest.version,
        pkg.manifest.protocol.driver,
        pkg.points.points.len()
    );

    let driver: Arc<dyn hmi_domain::driver::Driver> =
        Arc::new(hmi_driver_modbus_rtu::ModbusRtuDriver::new());
    if pkg.manifest.protocol.driver != driver.id() {
        bail!(
            "包声明的驱动 `{}` 与本命令支持的 `{}` 不一致",
            pkg.manifest.protocol.driver,
            driver.id()
        );
    }

    // 传输：SIM 或串口
    let bus: Box<dyn hmi_domain::transport::BusTransport> = if args.com.eq_ignore_ascii_case("SIM")
    {
        println!("总线: SIM（虚拟，预置演示数据）");
        let slave = crate::simnet::demo_slave();
        let (transport, endpoint) = hmi_driver_modbus_rtu::sim_pair();
        hmi_driver_modbus_rtu::spawn_slave_endpoint(endpoint, slave);
        Box::new(transport)
    } else {
        let cfg = SerialChannelConfig {
            id: "cli".into(),
            name: String::new(),
            port: args.com.clone(),
            hw_id: None,
            baud_rate: args.baud,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
            resp_timeout_ms: 300,
            retries: 1,
            inter_frame_gap_ms: 5,
        };
        println!(
            "总线: {} @ {} 8{}1，从站 {}",
            args.com,
            args.baud,
            cfg.parity.as_str().to_uppercase(),
            args.slave
        );
        Box::new(hmi_driver_modbus_rtu::SerialTransport::open(&cfg).map_err(anyhow::Error::msg)?)
    };

    let plan = hmi_gateway::poll::compile(
        "dev",
        &Default::default(),
        &pkg.points,
        &pkg.manifest.protocol.defaults,
    )?;
    let timeout = Duration::from_millis(300);

    println!("轮询周期: {}ms，Ctrl-C 退出\n", args.interval_ms);
    let mut cycle = 0u64;
    loop {
        cycle += 1;
        let t0 = std::time::Instant::now();
        println!("── 第 {cycle} 轮 ──");
        for g in &plan.groups {
            let r = driver
                .read_blocks(bus.as_ref(), args.slave, &g.blocks, timeout)
                .await;
            match r {
                Ok(data) => {
                    let batch = g.decode("dev", &data);
                    let mut sorted = batch;
                    sorted.sort_by_key(|a| a.0.to_string());
                    for (tag, snap) in sorted {
                        println!(
                            "  {tag:<24} {:>14}  [{:<9}] {}",
                            fmt_value(&snap.value),
                            snap.quality.to_string(),
                            g.name
                        );
                    }
                }
                Err(e) => {
                    println!("  [{}] 组 `{}` 读取失败: {e}", g.name, g.name);
                }
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
pub fn ensure_package_dir(p: &Path) -> Result<()> {
    if !p.is_dir() {
        bail!("设备包目录不存在: {}", p.display());
    }
    Ok(())
}
