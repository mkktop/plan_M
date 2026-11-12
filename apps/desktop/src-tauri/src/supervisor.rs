//! gatewayd 进程托管：应用启动时自动拉起采集服务，退出时只回收**自己拉起的**实例。
//!
//! 规则：
//! - 端口上已有 gatewayd（用户手动跑的 / 上次崩溃留下的孤儿）→ 附着使用，退出时不动它；
//! - 没有则从应用目录拉起 `gatewayd serve`（安装布局同目录；开发布局回落
//!   `target/release`），无窗口，日志本来就写数据目录 `logs/`；
//! - 应用退出（含更新重启）前终止自己拉起的子进程；
//! - 应用异常崩溃来不及回收时，孤儿子进程留在后台 —— 下次启动按「已有实例」附着，
//!   不会重复拉起，也不会丢采集。

use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 与 `gateway_info` 的默认端口保持一致。
pub const DEFAULT_PORT: u16 = 59120;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(8);
const PROBE_TIMEOUT: Duration = Duration::from_millis(400);

pub struct GatewaySupervisor {
    /// `Some` = 是我们拉起的实例，退出时要回收；`None` = 附着的外部实例。
    child: Option<Child>,
}

/// 由 `.manage()` 注入；回收需要 `&mut`，所以套 Mutex。
pub struct SupervisorState(pub Mutex<GatewaySupervisor>);

impl SupervisorState {
    /// gatewayd 是否本应用拉起（迁移数据目录要求可安全重启）。
    pub fn is_managed(&self) -> bool {
        self.0.lock().unwrap().child.is_some()
    }

    pub fn shutdown(&self) {
        self.0.lock().unwrap().shutdown();
    }

    /// 停掉当前实例并以新数据目录重新拉起（迁移数据目录用）。
    pub fn restart(&self, port: u16, data_dir: PathBuf) -> Result<(), String> {
        let mut guard = self.0.lock().unwrap();
        guard.shutdown();
        *guard = GatewaySupervisor::ensure(port, data_dir)?;
        Ok(())
    }
}

/// 端口探测用 TCP 连接即可：gatewayd 的 HTTP/WS 都监听这个端口。
fn port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, PROBE_TIMEOUT).is_ok()
}

/// 安装布局：gatewayd.exe 与应用同目录；开发布局：target/debug → target/release。
fn find_gatewayd() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let installed = exe_dir.join("gatewayd.exe");
    if installed.exists() {
        return Some(installed);
    }
    let dev = exe_dir.join("..").join("release").join("gatewayd.exe");
    dev.exists().then_some(dev)
}

impl GatewaySupervisor {
    pub fn ensure(port: u16, data_dir: PathBuf) -> Result<Self, String> {
        Self::ensure_with(port, data_dir, find_gatewayd)
    }

    /// `locate` 注入 gatewayd.exe 路径，便于单测模拟「找不到二进制」。
    pub fn ensure_with(
        port: u16,
        data_dir: PathBuf,
        locate: impl FnOnce() -> Option<PathBuf>,
    ) -> Result<Self, String> {
        if port_open(port) {
            return Ok(Self { child: None });
        }
        let exe = locate().ok_or_else(|| {
            "找不到 gatewayd.exe（浏览器调试或未随包分发时请手动 gatewayd serve）".to_string()
        })?;
        let mut command = Command::new(&exe);
        command
            .arg("serve")
            .arg("--data-dir")
            .arg(&data_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW：采集服务在后台跑，不弹控制台
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("启动 gatewayd 失败：{e}"))?;
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        while Instant::now() < deadline {
            if port_open(port) {
                return Ok(Self { child: Some(child) });
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        // 启动超时：不留半死进程
        let _ = child.kill();
        let _ = child.wait();
        Err("gatewayd 启动超时（8 秒内端口未就绪），请检查数据目录日志".into())
    }

    /// 只回收自己拉起的实例；用户手动运行的外部 gatewayd 不动。
    pub fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for GatewaySupervisor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attaches_to_existing_port() {
        // 端口上已有服务（这里用一个裸 TCP 监听模拟）→ 附着、不持有子进程
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let supervisor = GatewaySupervisor::ensure_with(port, PathBuf::from("."), || None).unwrap();
        assert!(supervisor.child.is_none());
    }

    #[test]
    fn missing_binary_is_reported_without_spawning() {
        // 端口 1 通常无监听且不会被占用成服务；找不到二进制必须报错而不是卡启动等待
        let err = GatewaySupervisor::ensure_with(1, PathBuf::from("."), || None)
            .err()
            .expect("missing binary should fail fast");
        assert!(err.contains("gatewayd.exe"), "unexpected error: {err}");
    }

    #[test]
    fn shutdown_is_idempotent_for_attached() {
        // 附着模式下 shutdown 是空操作，可以安全调用多次
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut supervisor =
            GatewaySupervisor::ensure_with(port, PathBuf::from("."), || None).unwrap();
        supervisor.shutdown();
        supervisor.shutdown();
        assert!(supervisor.child.is_none());
    }
}
