//! 桌面壳（thin）：只做 UI 宿主、连接信息读取与应用更新。
//!
//! 采集、协议、配置真值全部在 gatewayd；这里不实现任何业务逻辑。
//! UI 退出不杀 gatewayd —— 采集必须常驻。

pub mod updates;

use serde::Serialize;
use std::path::PathBuf;

/// UI 启动时拿到的 gatewayd 连接参数（对应 `ui/src/api/ws.ts` 的 GatewayInfo）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GatewayInfo {
    ws_url: String,
    token: Option<String>,
    token_path: String,
    data_dir: String,
    port: u16,
    host_version: String,
}

/// 读取 gatewayd 数据目录下的 token，拼出默认 WS 地址。
///
/// 只读文件，不做健康检查：连接状态由 UI 的 WebSocket 自己判断。
#[tauri::command]
fn gateway_info(port: Option<u16>, data_dir: Option<String>) -> GatewayInfo {
    let port = port.unwrap_or(59120);
    let dir = data_dir
        .map(PathBuf::from)
        .unwrap_or_else(default_gateway_dir);
    let token_path = dir.join("gateway.token");
    let token = std::fs::read_to_string(&token_path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    GatewayInfo {
        ws_url: format!("ws://127.0.0.1:{port}/ws"),
        token,
        token_path: token_path.display().to_string(),
        data_dir: dir.display().to_string(),
        port,
        host_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// 与 gatewayd 的 `default_data_dir()` 保持一致：`%APPDATA%/gatewayd`。
fn default_gateway_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gatewayd")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 文件/目录选择：设备包 zip 或解包目录 → 把绝对路径交给 gatewayd 导入。
        .plugin(tauri_plugin_dialog::init())
        .manage(updates::PendingUpdateState::new())
        .invoke_handler(tauri::generate_handler![
            gateway_info,
            updates::get_app_version,
            updates::check_for_updates,
            updates::download_update,
            updates::install_update,
            updates::open_update_release
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
