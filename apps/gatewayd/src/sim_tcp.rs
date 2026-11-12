//! `sim-tcp` 子命令：起一个本地 Modbus TCP 从站，便于无硬件验证 WiFi/TCP 通道。

use anyhow::Result;
use hmi_driver_modbus::serve_tcp_stream;
use std::net::SocketAddr;
use std::sync::Arc;

pub async fn run(listen: SocketAddr) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(listen).await?;
    println!("Modbus TCP 演示从站已启动: {listen}");
    println!(
        "在 UI 中新建 TCP 通道 → {}:{}，挂载任意 pm3000 设备包即可。",
        listen.ip(),
        listen.port()
    );
    let slave = crate::simnet::demo_slave();
    loop {
        let (stream, peer) = listener.accept().await?;
        let s: Arc<_> = slave.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_tcp_stream(stream, s).await {
                eprintln!("客户端 {peer} 断开: {e}");
            }
        });
    }
}
