//! scan 子命令：探测一条总线上有哪些从站应答。

use anyhow::Result;
use hmi_domain::config::{Parity, SerialChannelConfig};
use hmi_domain::driver::Driver;
use std::sync::Arc;
use std::time::Duration;

pub struct ScanArgs {
    pub com: String,
    pub baud: u32,
    pub from: u8,
    pub to: u8,
}

pub async fn run(args: ScanArgs) -> Result<()> {
    let cfg = SerialChannelConfig {
        id: "scan".into(),
        name: String::new(),
        port: args.com.clone(),
        hw_id: None,
        baud_rate: args.baud,
        data_bits: 8,
        parity: Parity::None,
        stop_bits: 1,
        resp_timeout_ms: 150,
        retries: 0,
        inter_frame_gap_ms: 2,
    };
    let bus = hmi_driver_modbus_rtu::SerialTransport::open(&cfg).map_err(anyhow::Error::msg)?;
    let driver = Arc::new(hmi_driver_modbus_rtu::ModbusRtuDriver::new());

    println!(
        "扫描 {} @ {}，从站 {}-{} …",
        args.com, args.baud, args.from, args.to
    );
    let mut found = Vec::new();
    let t0 = std::time::Instant::now();
    for slave in args.from..=args.to {
        print!("\r  探测从站 {slave:>3} …");
        let r = driver.probe(&bus, slave, Duration::from_millis(150)).await;
        if r.is_ok() {
            found.push(slave);
        }
        // 打印 flush
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
    println!("\r{}\n", " ".repeat(30));
    if found.is_empty() {
        println!("未发现任何从站（检查接线/波特率/从站地址）");
    } else {
        println!(
            "发现 {} 个从站: {:?}（耗时 {:?}）",
            found.len(),
            found,
            t0.elapsed()
        );
    }
    Ok(())
}
