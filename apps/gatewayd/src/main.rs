//! gatewayd —— 采集网关守护进程 / 调试 CLI。

use anyhow::Result;
use clap::{Parser, Subcommand};
use hmi_gatewayd::{cmd_pkg, cmd_poll, cmd_scan, default_data_dir, logging, serve};

#[derive(Parser)]
#[command(
    name = "gatewayd",
    version,
    about = "上位机采集网关：serve 常驻服务 / poll·scan 串口调试 / pkg 设备包管理"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 启动常驻服务（HTTP /health + WebSocket /ws）
    Serve {
        /// 监听端口
        #[arg(long, default_value_t = 59120)]
        port: u16,
        /// 数据目录（配置/包/日志）
        #[arg(long)]
        data_dir: Option<std::path::PathBuf>,
        /// 演示模式：自动装示例设备包 + 虚拟总线（无需硬件）
        #[arg(long)]
        sim: bool,
    },
    /// 直接轮询一个设备包并打印（调试利器，--com SIM 可无硬件运行）
    Poll {
        /// 设备包目录
        #[arg(long)]
        package: std::path::PathBuf,
        /// 串口名（如 COM3），或 SIM
        #[arg(long)]
        com: String,
        /// 从站地址
        #[arg(long, default_value_t = 1)]
        slave: u8,
        #[arg(long, default_value_t = 9600)]
        baud: u32,
        /// 轮询周期（毫秒）
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
        /// 只轮询一轮
        #[arg(long)]
        once: bool,
    },
    /// 扫描总线上的从站
    Scan {
        #[arg(long)]
        com: String,
        #[arg(long, default_value_t = 9600)]
        baud: u32,
        #[arg(long, default_value_t = 1)]
        from: u8,
        #[arg(long, default_value_t = 247)]
        to: u8,
    },
    /// 设备包管理
    Pkg {
        #[command(subcommand)]
        cmd: PkgCli,
    },
}

#[derive(Subcommand)]
enum PkgCli {
    /// 校验设备包（manifest + 点表）
    Validate { path: std::path::PathBuf },
    /// 安装设备包到数据目录
    Install {
        path: std::path::PathBuf,
        #[arg(long)]
        data_dir: Option<std::path::PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.cmd {
        Cmd::Serve {
            port,
            data_dir,
            sim,
        } => {
            let data_dir = data_dir.clone().unwrap_or_else(default_data_dir);
            std::fs::create_dir_all(&data_dir)?;
            let _guard = logging::init(&data_dir)?;

            let service = serve::build_service(&data_dir, *sim).await?;
            let token = read_or_create_token(&data_dir)?;
            let state = std::sync::Arc::new(serve::AppState {
                service,
                token: token.clone(),
            });
            let app = serve::build_router(state);

            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], *port));
            let listener = tokio::net::TcpListener::bind(addr).await?;
            println!("gatewayd {} 已启动", hmi_gatewayd::HOST_VERSION);
            println!("  数据目录 : {}", data_dir.display());
            println!("  健康检查 : http://127.0.0.1:{port}/health");
            println!("  WebSocket: ws://127.0.0.1:{port}/ws");
            println!(
                "  访问令牌 : {token}（亦见 {}/gateway.token）",
                data_dir.display()
            );
            if *sim {
                println!("  演示模式 : SIM 虚拟总线 + 示例设备包 com.acme.pm3000");
            }
            axum::serve(listener, app).await?;
            Ok(())
        }
        Cmd::Poll {
            package,
            com,
            slave,
            baud,
            interval_ms,
            once,
        } => {
            cmd_poll::run(cmd_poll::PollArgs {
                package: package.clone(),
                com: com.clone(),
                slave: *slave,
                baud: *baud,
                interval_ms: *interval_ms,
                once: *once,
            })
            .await
        }
        Cmd::Scan {
            com,
            baud,
            from,
            to,
        } => {
            cmd_scan::run(cmd_scan::ScanArgs {
                com: com.clone(),
                baud: *baud,
                from: *from,
                to: *to,
            })
            .await
        }
        Cmd::Pkg { cmd } => {
            let c = match cmd {
                PkgCli::Validate { path } => cmd_pkg::PkgCmd::Validate { path: path.clone() },
                PkgCli::Install { path, data_dir } => cmd_pkg::PkgCmd::Install {
                    path: path.clone(),
                    data_dir: data_dir.clone(),
                },
            };
            cmd_pkg::run(c).await
        }
    }
}

fn read_or_create_token(data_dir: &std::path::Path) -> Result<String> {
    let path = data_dir.join("gateway.token");
    if path.exists() {
        let t = std::fs::read_to_string(&path)?.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    let t = serve::new_token();
    std::fs::write(&path, &t)?;
    Ok(t)
}
