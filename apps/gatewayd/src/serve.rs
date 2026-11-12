//! serve 子命令：axum HTTP/WS 服务，UI（未来的 Tauri 壳）的唯一入口。

use crate::HOST_VERSION;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use hmi_domain::driver::Driver;
use hmi_domain::value::TagId;
use hmi_gateway::supervisor::GatewayEvent;
use hmi_gateway::GatewayService;
use hmi_wire::{ClientMsg, ServerMsg, UiPluginDto};
use semver::Version;
use std::collections::HashSet;
use std::sync::Arc;

pub struct AppState {
    pub service: Arc<GatewayService>,
    pub token: String,
}

pub type SharedState = Arc<AppState>;

/// 组装服务（含演示包/演示配置种子，`sim = true` 时）。
pub async fn build_service(
    data_dir: &std::path::Path,
    sim: bool,
) -> anyhow::Result<Arc<GatewayService>> {
    let drivers: Vec<Arc<dyn Driver>> =
        vec![Arc::new(hmi_driver_modbus_rtu::ModbusRtuDriver::new())];
    let service = GatewayService::new(
        data_dir,
        Version::parse(HOST_VERSION)?,
        drivers,
        crate::bus_opener(crate::simnet::SimNetwork::new()),
    )?;

    if sim {
        seed_demo(&service).await?;
    }
    service.start().await?;
    Ok(service)
}

/// 写入演示设备包（内嵌的 pm3000 示例）并配置一条 SIM 通道 + 两台设备。
async fn seed_demo(service: &Arc<GatewayService>) -> anyhow::Result<()> {
    const MANIFEST: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/packages/pm3000/manifest.json"
    ));
    const POINTS: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/packages/pm3000/points.json"
    ));
    const TREND_WIDGET: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/packages/pm3000/widgets/trend.js"
    ));

    let already = service.packages().await;
    if !already.iter().any(|p| p.id == "com.acme.pm3000") {
        // 物化到临时目录走正式安装流
        let seed_dir = std::env::temp_dir().join(format!(
            "hmi-demo-seed-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&seed_dir)?;
        std::fs::write(seed_dir.join("manifest.json"), MANIFEST)?;
        std::fs::write(seed_dir.join("points.json"), POINTS)?;
        std::fs::create_dir_all(seed_dir.join("widgets"))?;
        std::fs::write(seed_dir.join("widgets").join("trend.js"), TREND_WIDGET)?;
        let r = service.import_package(seed_dir.to_str().unwrap()).await;
        let _ = std::fs::remove_dir_all(&seed_dir);
        match r {
            Ok(p) => tracing::info!(package = %p.id, version = %p.version, "已安装演示设备包"),
            Err(e) => tracing::warn!(error = %e, "演示设备包安装失败（可能已存在）"),
        }
    }

    // 空配置 → 建演示通道与设备
    if service.snapshot().await.channels.is_empty() {
        service
            .upsert_channel(hmi_domain::config::SerialChannelConfig {
                id: "sim1".into(),
                name: "演示总线（虚拟）".into(),
                port: "SIM".into(),
                hw_id: None,
                baud_rate: 9600,
                data_bits: 8,
                parity: hmi_domain::config::Parity::None,
                stop_bits: 1,
                resp_timeout_ms: 150,
                retries: 2,
                inter_frame_gap_ms: 2,
            })
            .await?;
        for (id, slave, name) in [("pm1", 1u8, "演示电表 1"), ("pm2", 2u8, "演示电表 2")] {
            service
                .add_device(hmi_domain::config::DeviceConfig {
                    id: id.into(),
                    name: name.into(),
                    channel_id: "sim1".into(),
                    package: "com.acme.pm3000".into(),
                    slave,
                    overrides: Default::default(),
                })
                .await?;
        }
    }
    Ok(())
}

pub fn build_router(state: SharedState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_upgrade))
        // UI 插件：清单 + ESM/资源静态服务（webview 直接动态 import）。
        .route("/plugins", get(ui_plugin_list))
        .route("/plugins/{id}/{*path}", get(ui_plugin_asset))
        // 设备包 ESM 组件：只暴露清单声明 entry 所在目录下的文件。
        .route("/packages/{id}/{version}/{*path}", get(package_asset))
        .with_state(state)
}

async fn health() -> impl axum::response::IntoResponse {
    (
        // UI 运行在 tauri:// 或 vite dev server 源上，探测 health 需要放行跨源。
        [(
            axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
            axum::http::HeaderValue::from_static("*"),
        )],
        Json(serde_json::json!({
            "status": "ok",
            "name": "gatewayd",
            "version": HOST_VERSION,
        })),
    )
}

/// 插件清单（HTTP 版，便于外部工具/调试；UI 走 WS 的 `getUiPlugins`）。
async fn ui_plugin_list(State(state): State<SharedState>) -> Response {
    let plugins: Vec<UiPluginDto> = state.service.ui_plugins().await;
    (
        [(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        )],
        Json(plugins),
    )
        .into_response()
}

/// 插件静态资源：`/plugins/<id>/<entry>`。
///
/// webview 里 `import()` 是跨源请求，必须有 CORS；插件升级后 URL 带版本查询串，
/// 这里仍用 `no-cache` 保证开发期改文件后刷新即生效。
async fn ui_plugin_asset(
    Path((id, relative)): Path<(String, String)>,
    State(state): State<SharedState>,
) -> Response {
    if !valid_plugin_id_segment(&id) {
        return plugin_error(StatusCode::BAD_REQUEST, "非法插件 id");
    }
    let rel = std::path::Path::new(&relative);
    if rel.is_absolute()
        || rel
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return plugin_error(StatusCode::BAD_REQUEST, "非法插件资源路径");
    }

    let Some(base) = state.service.ui_plugin_dir(&id).await else {
        return plugin_error(StatusCode::NOT_FOUND, "插件未安装");
    };
    let file = base.join(rel);
    let Ok(canon_base) = std::fs::canonicalize(&base) else {
        return plugin_error(StatusCode::NOT_FOUND, "插件未安装");
    };
    let Ok(canon_file) = std::fs::canonicalize(&file) else {
        return plugin_error(StatusCode::NOT_FOUND, "插件资源不存在");
    };
    if !canon_file.starts_with(&canon_base) || !canon_file.is_file() {
        return plugin_error(StatusCode::FORBIDDEN, "插件资源路径越界");
    }

    let data = match tokio::fs::read(&canon_file).await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(file = %canon_file.display(), error = %e, "读取插件资源失败");
            return plugin_error(StatusCode::INTERNAL_SERVER_ERROR, "读取插件资源失败");
        }
    };
    (
        [
            (
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static(mime_of(&canon_file)),
            ),
        ],
        data,
    )
        .into_response()
}

fn plugin_error(status: StatusCode, message: &'static str) -> Response {
    (
        [(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        )],
        (status, message),
    )
        .into_response()
}

/// 设备包 ESM 组件资源：`/packages/<id>/<version>/<entry 或同目录文件>`。
///
/// 与服务层共用同一套越界/声明校验；未声明的路径一律 404（不暴露包内其他文件）。
async fn package_asset(
    Path((id, version, relative)): Path<(String, String, String)>,
    State(state): State<SharedState>,
) -> Response {
    let Some(file) = state
        .service
        .package_widget_asset(&id, &version, &relative)
        .await
    else {
        return plugin_error(StatusCode::NOT_FOUND, "设备包组件资源不存在或未声明");
    };
    let data = match tokio::fs::read(&file).await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(file = %file.display(), error = %e, "读取设备包组件资源失败");
            return plugin_error(StatusCode::INTERNAL_SERVER_ERROR, "读取设备包组件资源失败");
        }
    };
    (
        [
            (
                header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static(mime_of(&file)),
            ),
        ],
        data,
    )
        .into_response()
}

fn valid_plugin_id_segment(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn mime_of(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(state): State<SharedState>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// 单连接会话：一个任务内 select 三路 —— 客户端消息 / tag 变更 / 服务事件。
struct Session {
    state: SharedState,
    sub: Option<hmi_gateway::Subscription>,
    subscribed: HashSet<String>,
    authenticated: bool,
}

async fn handle_socket(mut socket: WebSocket, state: SharedState) {
    let mut session = Session {
        state: state.clone(),
        sub: None,
        subscribed: HashSet::new(),
        authenticated: false,
    };
    let mut events = state.service.subscribe_events();

    tracing::debug!("WS 连接建立");

    loop {
        tokio::select! {
            msg = socket.recv() => {
                let Some(Ok(msg)) = msg else { break };
                match msg {
                    Message::Text(text) => {
                        match session.handle_text(&text).await {
                            Ok(Some(server_msg)) => {
                                let j = serde_json::to_string(&server_msg).unwrap();
                                if socket.send(Message::Text(j.into())).await.is_err() {
                                    break;
                                }
                            }
                            Ok(None) => {}
                            Err(()) => {
                                // 未认证且第一条不是 hello / token 错误 → 踢掉
                                let _ = socket.send(Message::Text(
                                    serde_json::to_string(&ServerMsg::Error {
                                        message: "握手失败：需要正确的 hello/token".into(),
                                    })
                                    .unwrap()
                                    .into(),
                                ))
                                .await;
                                break;
                            }
                        }
                    }
                    Message::Ping(p) => {
                        let _ = socket.send(Message::Pong(p)).await;
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            Some(batch) = async {
                match &mut session.sub {
                    Some(s) => s.rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                // 合并窗口：再吸 15ms 内的批
                let mut items = batch;
                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                if let Some(s) = &mut session.sub {
                    while let Ok(more) = s.rx.try_recv() {
                        items.extend(more);
                    }
                }
                let j = serde_json::to_string(&ServerMsg::Changes { items }).unwrap();
                if socket.send(Message::Text(j.into())).await.is_err() {
                    break;
                }
            }
            ev = events.recv() => {
                match ev {
                    Ok(ev) => {
                        let j = serde_json::to_string(&event_to_msg(ev)).unwrap();
                        if socket.send(Message::Text(j.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "事件订阅滞后");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    if let Some(s) = session.sub.take() {
        state.service.hub.unsubscribe(s.id);
    }
    tracing::debug!("WS 连接关闭");
}

fn event_to_msg(ev: GatewayEvent) -> ServerMsg {
    match ev {
        GatewayEvent::ChannelState {
            channel,
            state,
            detail,
        } => ServerMsg::ChannelState {
            channel,
            state,
            detail,
        },
        GatewayEvent::DeviceOnline { device, online } => ServerMsg::DeviceState { device, online },
    }
}

impl Session {
    /// 处理一条文本消息。Err(()) = 握手失败，连接应断开。
    async fn handle_text(&mut self, text: &str) -> Result<Option<ServerMsg>, ()> {
        let msg: ClientMsg = match serde_json::from_str(text) {
            Ok(m) => m,
            Err(e) => {
                return Ok(Some(ServerMsg::Error {
                    message: format!("消息解析失败: {e}"),
                }))
            }
        };

        if !self.authenticated {
            // 未认证：只接受 hello
            let ClientMsg::Hello { token } = &msg else {
                return Err(());
            };
            if *token != self.state.token {
                return Err(());
            }
            self.authenticated = true;
            return Ok(Some(ServerMsg::Welcome {
                host_version: HOST_VERSION.into(),
            }));
        }

        let svc = self.state.service.clone();
        match msg {
            ClientMsg::Hello { .. } => Ok(Some(ServerMsg::Welcome {
                host_version: HOST_VERSION.into(),
            })),
            ClientMsg::GetSnapshot => Ok(Some(ServerMsg::Snapshot(svc.snapshot().await))),
            ClientMsg::Subscribe { tags } => {
                for t in tags {
                    self.subscribed.insert(t);
                }
                self.apply_subscription();
                // 立即回当前值（首屏）
                let ids: Vec<TagId> = self
                    .subscribed
                    .iter()
                    .map(|t| TagId::new(t.clone()))
                    .collect();
                let items = svc.hub.snapshot_of(&ids);
                Ok(Some(ServerMsg::Changes { items }))
            }
            ClientMsg::Unsubscribe { tags } => {
                for t in tags {
                    self.subscribed.remove(&t);
                }
                self.apply_subscription();
                Ok(Some(ServerMsg::CommandOk {
                    kind: "unsubscribe".into(),
                }))
            }
            ClientMsg::WriteTag { tag, value } => match svc.write_tag(&tag, value).await {
                Ok(()) => Ok(Some(ServerMsg::CommandOk {
                    kind: "writeTag".into(),
                })),
                Err(e) => Ok(Some(ServerMsg::Error { message: e })),
            },
            ClientMsg::ScanChannel { channel, range } => {
                let range = range.unwrap_or((1, 32));
                let found = svc.scan_channel(&channel, range).await;
                Ok(Some(ServerMsg::ScanDone { channel, found }))
            }
            ClientMsg::GetSerialPorts => {
                let ports = hmi_driver_modbus_rtu::list_serial_ports()
                    .into_iter()
                    .map(|p| hmi_wire::SerialPortDto {
                        port_name: p.port_name,
                        hw_id: p.hw_id,
                        description: p.description,
                    })
                    .collect();
                Ok(Some(ServerMsg::SerialPorts { ports }))
            }
            ClientMsg::GetPackages => {
                let packages = svc.packages().await;
                Ok(Some(ServerMsg::Packages { packages }))
            }
            ClientMsg::UpsertChannel { config } => match svc.upsert_channel(config).await {
                Ok(()) => Ok(Some(ServerMsg::Snapshot(svc.snapshot().await))),
                Err(e) => Ok(Some(ServerMsg::Error {
                    message: e.to_string(),
                })),
            },
            ClientMsg::RemoveChannel { channel } => match svc.remove_channel(&channel).await {
                Ok(()) => Ok(Some(ServerMsg::Snapshot(svc.snapshot().await))),
                Err(e) => Ok(Some(ServerMsg::Error {
                    message: e.to_string(),
                })),
            },
            ClientMsg::AddDevice { config } => match svc.add_device(config).await {
                Ok(()) => Ok(Some(ServerMsg::Snapshot(svc.snapshot().await))),
                Err(e) => Ok(Some(ServerMsg::Error {
                    message: e.to_string(),
                })),
            },
            ClientMsg::RemoveDevice { device } => match svc.remove_device(&device).await {
                Ok(()) => Ok(Some(ServerMsg::Snapshot(svc.snapshot().await))),
                Err(e) => Ok(Some(ServerMsg::Error {
                    message: e.to_string(),
                })),
            },
            ClientMsg::ImportPackage { path } => match svc.import_package(&path).await {
                Ok(_) => Ok(Some(ServerMsg::Snapshot(svc.snapshot().await))),
                Err(e) => Ok(Some(ServerMsg::Error { message: e })),
            },
            ClientMsg::RemovePackage { id } => match svc.remove_package(&id).await {
                Ok(()) => Ok(Some(ServerMsg::Snapshot(svc.snapshot().await))),
                Err(e) => Ok(Some(ServerMsg::Error { message: e })),
            },
            ClientMsg::GetDashboards => Ok(Some(ServerMsg::Dashboards {
                dashboards: svc.dashboards().await,
            })),
            ClientMsg::SaveDashboard { dashboard } => match svc.save_dashboard(dashboard).await {
                Ok(dashboards) => Ok(Some(ServerMsg::Dashboards { dashboards })),
                Err(e) => Ok(Some(ServerMsg::Error { message: e })),
            },
            ClientMsg::RemoveDashboard { id } => match svc.remove_dashboard(&id).await {
                Ok(dashboards) => Ok(Some(ServerMsg::Dashboards { dashboards })),
                Err(e) => Ok(Some(ServerMsg::Error { message: e })),
            },
            ClientMsg::GetUiPlugins => Ok(Some(ServerMsg::UiPlugins {
                plugins: svc.ui_plugins().await,
            })),
            ClientMsg::ImportUiPlugin { path } => match svc.import_ui_plugin(&path).await {
                Ok(plugins) => Ok(Some(ServerMsg::UiPlugins { plugins })),
                Err(e) => Ok(Some(ServerMsg::Error { message: e })),
            },
            ClientMsg::RemoveUiPlugin { id } => match svc.remove_ui_plugin(&id).await {
                Ok(plugins) => Ok(Some(ServerMsg::UiPlugins { plugins })),
                Err(e) => Ok(Some(ServerMsg::Error { message: e })),
            },
            ClientMsg::GetWorkspaces => Ok(Some(ServerMsg::Workspaces {
                workspaces: svc.workspaces().await,
            })),
            ClientMsg::SaveWorkspace { workspace } => match svc.save_workspace(workspace).await {
                Ok(workspaces) => Ok(Some(ServerMsg::Workspaces { workspaces })),
                Err(e) => Ok(Some(ServerMsg::Error { message: e })),
            },
            ClientMsg::RemoveWorkspace { device_type } => {
                match svc.remove_workspace(&device_type).await {
                    Ok(workspaces) => Ok(Some(ServerMsg::Workspaces { workspaces })),
                    Err(e) => Ok(Some(ServerMsg::Error { message: e })),
                }
            }
            ClientMsg::ValidatePackage { manifest, points } => {
                match svc.validate_package_draft(&manifest, &points) {
                    Ok(issues) => Ok(Some(ServerMsg::PackageValidation {
                        errors: issues.errors,
                        warnings: issues.warnings,
                    })),
                    Err(e) => Ok(Some(ServerMsg::Error { message: e })),
                }
            }
            ClientMsg::InstallDraftPackage { manifest, points } => {
                match svc.install_draft_package(&manifest, &points).await {
                    Ok(_) => Ok(Some(ServerMsg::Snapshot(svc.snapshot().await))),
                    Err(e) => Ok(Some(ServerMsg::Error { message: e })),
                }
            }
            ClientMsg::ExportPackage {
                manifest,
                points,
                dir,
            } => match svc.export_package_draft(&manifest, &points, &dir) {
                Ok(()) => Ok(Some(ServerMsg::CommandOk {
                    kind: "exportPackage".into(),
                })),
                Err(e) => Ok(Some(ServerMsg::Error { message: e })),
            },
        }
    }

    fn apply_subscription(&mut self) {
        let ids: Vec<TagId> = self
            .subscribed
            .iter()
            .map(|t| TagId::new(t.clone()))
            .collect();
        match &mut self.sub {
            Some(s) => self.state.service.hub.update_filter(s.id, ids),
            None => {
                if !self.subscribed.is_empty() {
                    self.sub = Some(self.state.service.hub.subscribe(ids));
                }
            }
        }
    }
}

/// 供 main 使用：生成 token。
pub fn new_token() -> String {
    uuid::Uuid::new_v4().to_string()
}
