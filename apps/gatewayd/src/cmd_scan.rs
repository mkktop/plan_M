//! scan 子命令：探测一条串口/TCP 链路上有哪些设备地址应答。

use anyhow::Result;
use hmi_domain::config::DeviceAddress;
use hmi_domain::protocol::ProtocolDecl;
use std::sync::Arc;
use std::time::Duration;

use crate::cmd_poll::{build_channel, factory_for, transport_summary, TransportArg};

pub struct ScanArgs {
    pub transport: TransportArg,
    pub com: Option<String>,
    pub host: Option<String>,
    pub tcp_port: u16,
    pub baud: u32,
    pub from: u32,
    pub to: u32,
}

pub async fn run(args: ScanArgs) -> Result<()> {
    let channel = build_channel(
        "scan",
        args.transport,
        args.com.as_deref(),
        args.host.as_deref(),
        args.tcp_port,
        args.baud,
        150,
    )?;
    let factory = factory_for(args.transport);
    let link = factory
        .open(&channel)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let driver: Arc<dyn hmi_domain::driver::Driver> = crate::drivers()
        .into_iter()
        .find(|d| d.id() == "modbus")
        .expect("宿主必须注册 modbus 驱动");
    let protocol = ProtocolDecl {
        driver: "modbus".into(),
        transports: Vec::new(),
        params: serde_json::json!({}),
    };

    println!(
        "扫描 {} {}，地址 {}-{} …",
        args.transport.kind(),
        transport_summary(&channel),
        args.from,
        args.to
    );
    let mut found = Vec::new();
    let t0 = std::time::Instant::now();
    for n in args.from..=args.to {
        print!("\r  探测地址 {n:>3} …");
        let addr = DeviceAddress::Number(n);
        if driver
            .probe(link.as_ref(), &addr, &protocol, Duration::from_millis(150))
            .await
            .is_ok()
        {
            found.push(n);
        }
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
    println!("\r{}\n", " ".repeat(30));
    if found.is_empty() {
        println!("未发现任何设备（检查接线/波特率/地址）");
    } else {
        println!(
            "发现 {} 个设备: {:?}（耗时 {:?}）",
            found.len(),
            found,
            t0.elapsed()
        );
    }
    Ok(())
}
