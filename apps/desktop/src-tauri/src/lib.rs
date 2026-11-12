//! 桌面壳（thin）：UI 宿主、gatewayd 进程托管、连接信息读取、数据目录管理与应用更新。
//!
//! 采集、协议、配置真值全部在 gatewayd；这里不实现任何业务逻辑。
//! 桌面壳会自动拉起/回收 gatewayd（见 `supervisor`）——但只管理自己拉起的实例，
//! 用户手动运行的外部 gatewayd 一律附着、不接管、不杀死。
//! 数据目录由 `desksettings` 的指针决定（默认 `%APPDATA%\gatewayd`，设置页可迁移）。

pub mod desksettings;
pub mod storage;
pub mod supervisor;
pub mod updates;

use serde::Serialize;
use std::path::PathBuf;
use tauri::Manager as _;

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

/// 读取数据目录下的 token，拼出默认 WS 地址。
///
/// 目录解析顺序：显式参数 > desktop.json 指针 > 默认 `%APPDATA%\gatewayd`
/// （与 supervisor 拉起 gatewayd 用的目录保持一致）。
/// 只读文件，不做健康检查：连接状态由 UI 的 WebSocket 自己判断。
#[tauri::command]
fn gateway_info(port: Option<u16>, data_dir: Option<String>) -> GatewayInfo {
    let port = port.unwrap_or(59120);
    let dir = data_dir
        .map(PathBuf::from)
        .unwrap_or_else(desksettings::resolve_data_dir);
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // 文件/目录选择：设备包 zip 或解包目录 → 把绝对路径交给 gatewayd 导入。
        .plugin(tauri_plugin_dialog::init())
        .manage(updates::PendingUpdateState::new())
        // UI 加载前确保采集服务在线：已有则附着，没有则按解析出的数据目录拉起。
        .setup(|app| {
            let data_dir = desksettings::resolve_data_dir();
            match supervisor::GatewaySupervisor::ensure(supervisor::DEFAULT_PORT, data_dir) {
                Ok(s) => {
                    app.manage(supervisor::SupervisorState(std::sync::Mutex::new(s)));
                }
                Err(e) => {
                    // 不阻断 UI：诊断/设置页会显示连接失败与手动启动指引
                    eprintln!("[supervisor] {e}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            gateway_info,
            storage::get_storage_info,
            storage::migrate_data_dir,
            updates::get_app_version,
            updates::check_for_updates,
            updates::download_update,
            updates::install_update,
            updates::open_update_release
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application");
    app.run(|app_handle, event| {
        // 退出（含安装重启 app.exit）前回收自己拉起的 gatewayd；附着的实例不动
        if let tauri::RunEvent::ExitRequested { .. } = event {
            if let Some(state) = app_handle.try_state::<supervisor::SupervisorState>() {
                state.0.lock().unwrap().shutdown();
            }
        }
    });
}
