//! 包编辑器草稿链路：校验 → 安装 → 导出（P4）。

use hmi_domain::driver::Driver;
use hmi_gateway::GatewayService;
use semver::Version;
use serde_json::json;
use std::sync::Arc;

fn service(dir: &std::path::Path) -> Arc<GatewayService> {
    let drivers: Vec<Arc<dyn Driver>> = vec![Arc::new(hmi_driver_modbus::ModbusDriver::new())];
    GatewayService::new(dir, Version::new(0, 1, 1), drivers, Vec::new()).unwrap()
}

fn manifest() -> serde_json::Value {
    json!({
        "schemaVersion": 2,
        "id": "com.test.studio",
        "name": "编辑器示例",
        "vendor": "测试",
        "version": "0.1.0",
        "protocol": { "driver": "modbus" },
        "ui": {
            "pages": [{
                "schemaVersion": 2,
                "id": "main",
                "title": "主页面",
                "icon": "activity",
                "layout": { "cols": 12, "rowHeight": 72, "gap": 12 },
                "widgets": [
                    { "id": "w1", "kind": "core:value", "title": "电压",
                      "rect": { "x": 0, "y": 0, "w": 3, "h": 2 },
                      "bind": { "value": { "point": "uab" } } },
                    { "id": "w2", "kind": "core:group", "title": "组合",
                      "rect": { "x": 3, "y": 0, "w": 6, "h": 4 },
                      "config": { "items": [
                          { "kind": "core:value", "title": "电流",
                            "rect": { "x": 0, "y": 0, "w": 12, "h": 2 },
                            "bind": { "value": { "point": "ia" } } }
                      ] } }
                ]
            }]
        }
    })
}

fn points() -> serde_json::Value {
    json!({
        "pollGroups": [{ "name": "fast", "intervalMs": 1000 }],
        "points": [
            { "id": "uab", "name": "AB线电压", "address": "40101", "type": "u16",
              "scale": 0.1, "unit": "V", "pollGroup": "fast" },
            { "id": "ia", "name": "A相电流", "address": "40102", "type": "u16",
              "scale": 0.01, "unit": "A", "pollGroup": "fast" }
        ]
    })
}

#[tokio::test]
async fn draft_validate_install_and_export() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());

    let issues = svc.validate_package_draft(&manifest(), &points()).unwrap();
    assert!(issues.ok(), "{issues:?}");

    let installed = svc
        .install_draft_package(&manifest(), &points())
        .await
        .unwrap();
    assert_eq!(installed.id, "com.test.studio");
    assert_eq!(installed.ui.pages.len(), 1);
    let page = &installed.ui.pages[0];
    assert_eq!(page.components.len(), 2);
    // v2 widgets[] 已翻译为 components，实例 id 保留
    assert_eq!(page.components[0].id.as_deref(), Some("w1"));
    assert_eq!(page.components[1].kind, "core:group");

    assert!(svc
        .packages()
        .await
        .iter()
        .any(|p| p.id == "com.test.studio"));

    // 导出保留原始 v2 形态（widgets[] 不被翻译写回）
    let out = tmp.path().join("export");
    svc.export_package_draft(&manifest(), &points(), out.to_str().unwrap())
        .unwrap();
    let text = std::fs::read_to_string(out.join("manifest.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["ui"]["pages"][0]["schemaVersion"], 2);
    assert_eq!(v["ui"]["pages"][0]["widgets"][0]["id"], "w1");
    assert!(out.join("points.json").is_file());
}

#[tokio::test]
async fn invalid_draft_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());

    let mut bad = manifest();
    bad["ui"]["pages"][0]["widgets"][0]["kind"] = json!("shell:evil");
    let issues = svc.validate_package_draft(&bad, &points()).unwrap();
    assert!(!issues.ok());
    assert!(
        issues.errors.iter().any(|e| e.contains("未知组件")),
        "{issues:?}"
    );

    let err = svc
        .install_draft_package(&bad, &points())
        .await
        .unwrap_err();
    assert!(err.contains("未知组件"), "{err}");

    // 未安装任何包
    assert!(svc.packages().await.is_empty());
}

#[tokio::test]
async fn draft_export_refuses_invalid_points() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());
    let mut bad_points = points();
    bad_points["points"][0]["address"] = json!("");
    let out = tmp.path().join("export");
    assert!(svc
        .export_package_draft(&manifest(), &bad_points, out.to_str().unwrap())
        .is_err());
    assert!(!out.join("manifest.json").exists());
}
