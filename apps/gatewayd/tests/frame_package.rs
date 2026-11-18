use hmi_domain::config::{ChannelConfig, ChannelTiming, DeviceConfig, TcpParams, TransportConfig};
use hmi_domain::protocol::ScanRange;
use hmi_domain::value::{Quality, Value};
use hmi_gateway::GatewayService;
use semver::Version;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn installed_frame_package_discovers_polls_and_writes_over_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let emulator = tokio::spawn(async move {
        let mut stored = 250u16.to_be_bytes();
        while let Ok((mut client, _)) = listener.accept().await {
            let mut header = [0; 2];
            while client.read_exact(&mut header).await.is_ok() {
                match header {
                    [7, 0x10] => {
                        if client.write_all(&[7, 0x90]).await.is_err() {
                            break;
                        }
                        tokio::task::yield_now().await;
                        if client.write_all(&stored).await.is_err() {
                            break;
                        }
                    }
                    [7, 0x11] => {
                        if client.read_exact(&mut stored).await.is_err() {
                            break;
                        }
                        if client.write_all(&[7, 0x91]).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    let tmp = tempfile::tempdir().unwrap();
    let gateway = GatewayService::new(
        tmp.path(),
        Version::new(0, 1, 2),
        hmi_gatewayd::drivers(),
        hmi_gatewayd::transports(),
    )
    .unwrap();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/packages/fixed-frame-meter");
    let pack = gateway
        .import_package(source.to_str().unwrap())
        .await
        .unwrap();
    gateway
        .upsert_channel(ChannelConfig {
            id: "bytebus".into(),
            name: "固定帧测试".into(),
            timing: ChannelTiming {
                resp_timeout_ms: 100,
                ..Default::default()
            },
            transport: TransportConfig::Tcp(TcpParams {
                host: "127.0.0.1".into(),
                port,
                connect_timeout_ms: 300,
            }),
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(60)).await;
    let found = gateway
        .scan_channel(
            "bytebus",
            None,
            Some(&pack.manifest.id),
            ScanRange { from: 6, to: 7 },
        )
        .await
        .unwrap();
    assert_eq!(
        found
            .iter()
            .filter_map(|address| address.as_number())
            .collect::<Vec<_>>(),
        vec![7]
    );
    gateway
        .add_device(DeviceConfig {
            id: "frame1".into(),
            name: "测试表".into(),
            channel_id: "bytebus".into(),
            package: pack.manifest.id,
            address: found[0].clone(),
            overrides: Default::default(),
        })
        .await
        .unwrap();

    async fn readback(service: &GatewayService, value: f64) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if service
                    .current_value("frame1.setpoint")
                    .is_some_and(|data| {
                        data.quality == Quality::Good && data.value.as_f64() == Some(value)
                    })
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
    }
    readback(&gateway, 125.0).await;
    gateway
        .write_tag("frame1.setpoint", Value::F64(150.5))
        .await
        .unwrap();
    readback(&gateway, 150.5).await;
    emulator.abort();
}
