//! WS 冒烟测试：起真实 gatewayd 服务（SIM 演示模式）→ 客户端走完整协议。
//!
//! 覆盖：hello/token 握手 → GetSnapshot → Subscribe → 收到 Good 值 Changes →
//! WriteTag → CommandOk。

use futures::{SinkExt, StreamExt};
use hmi_wire::{
    ClientMsg, DashboardDto, DevicePageComponentDto, DevicePageDto, ServerMsg, WidgetDto,
    WidgetLayout, WorkspaceDto,
};
use tokio_tungstenite::tungstenite::Message;

async fn start_server() -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    // tempdir 需活到服务结束 —— 泄漏到测试进程结束（测试短命，可接受）
    let dir = Box::leak(Box::new(dir));

    let service = hmi_gatewayd::serve::build_service(dir.path(), true)
        .await
        .expect("build service");
    let token = "test-token".to_string();
    let state = std::sync::Arc::new(hmi_gatewayd::serve::AppState {
        service,
        token: token.clone(),
    });
    let app = hmi_gatewayd::serve::build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("ws://{addr}/ws"), token)
}

async fn send(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    msg: &ClientMsg,
) {
    let j = serde_json::to_string(msg).unwrap();
    ws.send(Message::Text(j.into())).await.unwrap();
}

async fn recv(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> ServerMsg {
    loop {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
            .await
            .expect("等待服务端消息超时")
            .expect("连接中断")
            .expect("ws 错误");
        if let Message::Text(t) = msg {
            let m: ServerMsg = serde_json::from_str(t.as_str()).unwrap();
            eprintln!("RECV: {m:?}");
            return m;
        }
    }
}

#[tokio::test]
async fn ws_full_protocol_smoke() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("hmi_gatewayd=debug,hmi_gateway=debug")
        .try_init();
    let (url, token) = start_server().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("连接 gatewayd");

    // 1. 握手
    send(&mut ws, &ClientMsg::Hello { token }).await;
    match recv(&mut ws).await {
        ServerMsg::Welcome { host_version } => {
            assert!(!host_version.is_empty());
        }
        other => panic!("期望 Welcome，得到 {other:?}"),
    }

    // 2. 快照：应含 SIM 通道与两台演示设备、演示包
    send(&mut ws, &ClientMsg::GetSnapshot).await;
    let snap = loop {
        if let ServerMsg::Snapshot(s) = recv(&mut ws).await {
            break s;
        }
    };
    assert_eq!(snap.channels.len(), 1);
    assert_eq!(snap.channels[0].port, "SIM");
    assert_eq!(snap.devices.len(), 2);
    assert!(snap.packages.iter().any(|p| p.id == "com.acme.pm3000"));
    let pm = snap
        .packages
        .iter()
        .find(|p| p.id == "com.acme.pm3000")
        .unwrap();
    assert_eq!(pm.ui.pages.len(), 4, "演示包应声明四页工作区");
    assert_eq!(pm.ui.pages[0].components[0].kind, "overview");

    // 3. 订阅 pm1.uab，等待第一个 Good 值（模拟从站预置 2201 → 220.1V）
    send(
        &mut ws,
        &ClientMsg::Subscribe {
            tags: vec!["pm1.uab".into()],
        },
    )
    .await;
    let mut saw_good = false;
    for _ in 0..200 {
        if let ServerMsg::Changes { items } = recv(&mut ws).await {
            if let Some(v) = items.iter().find(|c| c.tag.as_str() == "pm1.uab") {
                if v.quality == hmi_domain::value::Quality::Good {
                    let f = v.value.as_f64().expect("数值");
                    assert!((f - 220.1).abs() < 1e-6, "电压应为 220.1，实得 {f}");
                    saw_good = true;
                    break;
                }
            }
        }
    }
    assert!(saw_good, "订阅后未收到 Good 的 pm1.uab");

    // 4. 写点：setpoint 可写 → CommandOk
    send(
        &mut ws,
        &ClientMsg::WriteTag {
            tag: "pm1.setpoint".into(),
            value: hmi_domain::value::Value::I64(300),
        },
    )
    .await;
    let mut wrote = false;
    for _ in 0..100 {
        if let ServerMsg::CommandOk { kind } = recv(&mut ws).await {
            assert_eq!(kind, "writeTag");
            wrote = true;
            break;
        }
    }
    assert!(wrote, "写点未收到 CommandOk");

    // 5. 只读点写入应报错
    send(
        &mut ws,
        &ClientMsg::WriteTag {
            tag: "pm1.uab".into(),
            value: hmi_domain::value::Value::I64(1),
        },
    )
    .await;
    let mut got_error = false;
    for _ in 0..100 {
        if let ServerMsg::Error { message } = recv(&mut ws).await {
            assert!(message.contains("只读"), "错误信息应说明只读: {message}");
            got_error = true;
            break;
        }
    }
    assert!(got_error, "只读点写入应返回错误");

    // 6. 仪表盘：保存 → 列表应答 → 删除
    let dashboard = DashboardDto {
        id: "main".into(),
        name: "总览".into(),
        cols: 12,
        scope_kind: None,
        scope_id: None,
        widgets: vec![WidgetDto {
            id: "w1".into(),
            kind: "value".into(),
            title: "AB 线电压".into(),
            tag: "pm1.uab".into(),
            unit: "V".into(),
            config: serde_json::json!({ "decimals": 1 }),
            layout: WidgetLayout {
                x: 0,
                y: 0,
                w: 3,
                h: 2,
            },
        }],
    };
    send(
        &mut ws,
        &ClientMsg::SaveDashboard {
            dashboard: dashboard.clone(),
        },
    )
    .await;
    let saved = loop {
        if let ServerMsg::Dashboards { dashboards } = recv(&mut ws).await {
            break dashboards;
        }
    };
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0], dashboard);

    send(&mut ws, &ClientMsg::RemoveDashboard { id: "main".into() }).await;
    let left = loop {
        if let ServerMsg::Dashboards { dashboards } = recv(&mut ws).await {
            break dashboards;
        }
    };
    assert!(left.is_empty(), "删除后应无仪表盘");

    // 7. 设备类型工作区：保存 → 列表应答 → 删除
    let workspace = WorkspaceDto {
        device_type: "com.acme.pm3000".into(),
        pages: vec![
            DevicePageDto {
                id: "overview".into(),
                title: "设备概览".into(),
                icon: "pulse".into(),
                components: vec![DevicePageComponentDto {
                    kind: "overview".into(),
                    title: String::new(),
                    config: serde_json::json!({}),
                }],
            },
            DevicePageDto {
                id: "my-page".into(),
                title: "我的页面".into(),
                icon: "grid".into(),
                components: vec![
                    DevicePageComponentDto {
                        kind: "tags".into(),
                        title: "点表".into(),
                        config: serde_json::json!({}),
                    },
                    DevicePageComponentDto {
                        kind: "dashboard".into(),
                        title: String::new(),
                        config: serde_json::json!({}),
                    },
                ],
            },
        ],
    };
    send(
        &mut ws,
        &ClientMsg::SaveWorkspace {
            workspace: workspace.clone(),
        },
    )
    .await;
    let saved = loop {
        if let ServerMsg::Workspaces { workspaces } = recv(&mut ws).await {
            break workspaces;
        }
    };
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0], workspace);

    send(
        &mut ws,
        &ClientMsg::RemoveWorkspace {
            device_type: "com.acme.pm3000".into(),
        },
    )
    .await;
    let left = loop {
        if let ServerMsg::Workspaces { workspaces } = recv(&mut ws).await {
            break workspaces;
        }
    };
    assert!(left.is_empty(), "删除覆盖后应无工作区记录");

    // 8. UI 插件：安装示例插件 → 列表应答 → 删除
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/ui-plugins/meter-card")
        .canonicalize()
        .expect("示例插件目录应存在");
    send(
        &mut ws,
        &ClientMsg::ImportUiPlugin {
            path: example.to_string_lossy().into_owned(),
        },
    )
    .await;
    let plugins = loop {
        if let ServerMsg::UiPlugins { plugins } = recv(&mut ws).await {
            break plugins;
        }
    };
    let plugin = plugins
        .iter()
        .find(|p| p.id == "com.example.meter-card")
        .expect("示例插件应安装成功");
    assert!(plugin.permissions.iter().any(|p| p == "tags:subscribe"));
    assert!(plugin.widgets.iter().any(|w| w.id == "meter"));
    assert_eq!(plugin.entry, "index.js");

    send(
        &mut ws,
        &ClientMsg::RemoveUiPlugin {
            id: "com.example.meter-card".into(),
        },
    )
    .await;
    let left = loop {
        if let ServerMsg::UiPlugins { plugins } = recv(&mut ws).await {
            break plugins;
        }
    };
    assert!(left.is_empty(), "删除后应无 UI 插件");
}

#[tokio::test]
async fn wrong_token_rejected() {
    let (url, _token) = start_server().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();
    send(
        &mut ws,
        &ClientMsg::Hello {
            token: "wrong".into(),
        },
    )
    .await;
    // 服务端应回 Error 后关闭连接
    let mut closed_or_error = false;
    for _ in 0..5 {
        match tokio::time::timeout(std::time::Duration::from_secs(2), ws.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => {
                let m: ServerMsg = serde_json::from_str(t.as_str()).unwrap();
                if matches!(m, ServerMsg::Error { .. }) {
                    closed_or_error = true;
                }
            }
            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) => {
                closed_or_error = true;
                break;
            }
            _ => {}
        }
    }
    assert!(closed_or_error, "错误 token 应被拒绝");
}
