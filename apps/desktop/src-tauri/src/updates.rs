//! 应用更新：读 Release 附带的 latest.json → 流式下载安装包（校验 sha256）→ 拉起安装程序后退出。
//!
//! 自研命令而非 tauri-plugin-updater：不需要签名密钥与 endpoints 托管，
//! 信任边界完全可控（只允许本仓库 Release 的 https 直链 + 平台后缀 + 资产摘要）。
//! 元数据走 `releases/latest/download/latest.json` 静态资产而非 api.github.com ——
//! 后者限流 60 次/小时/IP，更新检查可能拿到 403。UI 侧入口在「设置 → 应用与更新」。

use futures::StreamExt;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::ipc::Channel;

const REPO_RELEASES_URL: &str = "https://github.com/mkktop/plan_M/releases";
/// 静态清单资产，随 Release 由 release.yml 生成（CDN 有缓存，发布后最长延迟约 1 小时）。
const UPDATE_MANIFEST_URL: &str =
    "https://github.com/mkktop/plan_M/releases/latest/download/latest.json";
/// 下载直链只信任本仓库 Release 资产路径。
const TRUSTED_DOWNLOAD_PREFIX: &str = "/mkktop/plan_M/releases/download/";
const MAX_INSTALLER_SIZE: u64 = 512 * 1024 * 1024;
const USER_AGENT: &str = concat!("hmi-desktop/", env!("CARGO_PKG_VERSION"));

// —— 更新清单数据形状（scripts/release-artifacts.cjs 生成）——

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    installers: BTreeMap<String, ManifestInstaller>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestInstaller {
    #[serde(default)]
    name: String,
    url: String,
    size: u64,
    /// 纯 hex（生成端已去掉 sha256: 前缀）。
    #[serde(default)]
    sha256: String,
}

// —— 命令返回形状（前端 `stores/updates.ts` 同构）——

#[derive(Debug, Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum UpdateStatus {
    Available,
    Current,
    Unpublished,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum InstallerKind {
    Windows,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct UpdateCheck {
    status: UpdateStatus,
    current_version: String,
    version: Option<String>,
    notes: Option<String>,
    published_at: Option<String>,
    can_install: bool,
    download_size: Option<u64>,
    installer_kind: Option<InstallerKind>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct UpdateProgress {
    downloaded: u64,
    total: u64,
}

/// 一次待安装更新的全部状态：check 时记下来源与摘要，install 前复验。
#[derive(Debug, Clone)]
struct InstallerSpec {
    url: String,
    sha256: String,
    size: u64,
}

#[derive(Debug, Clone)]
struct PendingUpdate {
    spec: InstallerSpec,
    path: PathBuf,
}

/// 由 `run()` 里 `.manage()` 注入。
pub struct PendingUpdateState(Mutex<Option<PendingUpdate>>);

impl Default for PendingUpdateState {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

impl PendingUpdateState {
    pub fn new() -> Self {
        Self::default()
    }
}

// —— 判定逻辑（纯函数，便于单测）——

fn release_version(tag: &str) -> Option<Version> {
    let raw = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
    Version::parse(raw).ok()
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 当前发布只出 Windows NSIS 包；其他平台先返回 None（应用内不可装，只能去发布页）。
/// 键与 release-artifacts.cjs 的平台名一致；命名与 tauri 产物一致：
/// <productName>_<version>_x64-setup.exe
fn installer_platform() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("windows-x64")
    } else {
        None
    }
}

fn installer_suffix() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("_x64-setup.exe")
    } else {
        None
    }
}

fn installer_spec(installer: &ManifestInstaller) -> Option<InstallerSpec> {
    let suffix = installer_suffix()?;
    // 清单可能被篡改：资产名后缀、大小、摘要、URL 信任全部独立复核
    if !installer.name.ends_with(suffix) {
        return None;
    }
    if installer.size == 0 || installer.size > MAX_INSTALLER_SIZE {
        return None;
    }
    if !is_hex64(&installer.sha256) {
        return None;
    }
    if !trusted_download(&installer.url) {
        return None;
    }
    Some(InstallerSpec {
        url: installer.url.clone(),
        sha256: installer.sha256.clone(),
        size: installer.size,
    })
}

fn evaluate_manifest(manifest: &UpdateManifest, current: &Version) -> UpdateCheck {
    let mut check = UpdateCheck {
        status: UpdateStatus::Current,
        current_version: current.to_string(),
        version: None,
        notes: manifest.notes.clone(),
        published_at: manifest.published_at.clone(),
        can_install: false,
        download_size: None,
        installer_kind: None,
    };
    // 版本号解析不出来或带预发布段（rc/beta）→ 不提示更新
    let Some(version) = release_version(&manifest.version) else {
        return check;
    };
    check.version = Some(version.to_string());
    if !version.pre.is_empty() || !version.gt(current) {
        return check;
    }
    check.status = UpdateStatus::Available;
    if let Some(spec) = installer_platform()
        .and_then(|key| manifest.installers.get(key))
        .and_then(installer_spec)
    {
        check.can_install = true;
        check.download_size = Some(spec.size);
        check.installer_kind = Some(InstallerKind::Windows);
    }
    check
}

/// 只允许 https + github.com（无端口/用户信息/查询串/锚点）+ 本仓库 Release 资产路径，
/// 杜绝清单被指向任意可执行文件。
fn trusted_download(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "https"
        && parsed.host_str() == Some("github.com")
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.port().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
        && parsed.path().starts_with(TRUSTED_DOWNLOAD_PREFIX)
}

// —— 下载与校验 ——

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn verify_installer(bytes: &[u8], spec: &InstallerSpec) -> Result<(), String> {
    if bytes.len() as u64 != spec.size {
        return Err(format!(
            "安装包大小不符：期望 {} 字节，实际 {}",
            spec.size,
            bytes.len()
        ));
    }
    let digest = sha256_bytes(bytes);
    if !digest.eq_ignore_ascii_case(&spec.sha256) {
        return Err("安装包 sha256 校验失败，已放弃本次更新".into());
    }
    Ok(())
}

/// 流式下载并校验；progress 回调收到 (已下载, 总量)。
async fn download_installer(
    client: &reqwest::Client,
    spec: &InstallerSpec,
    dest: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let response = client
        .get(&spec.url)
        .send()
        .await
        .map_err(|e| format!("下载失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!("下载失败：HTTP {}", response.status()));
    }
    let total = response.content_length().unwrap_or(spec.size);
    if total > MAX_INSTALLER_SIZE {
        return Err("安装包超出大小上限".into());
    }
    let mut downloaded: u64 = 0;
    let mut hasher = Sha256::new();
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("无法写入临时文件：{e}"))?;
    use tokio::io::AsyncWriteExt;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断：{e}"))?;
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入临时文件失败：{e}"))?;
        downloaded += chunk.len() as u64;
        progress(downloaded, total);
    }
    file.flush()
        .await
        .map_err(|e| format!("写入临时文件失败：{e}"))?;
    drop(file);

    let digest = format!("{:x}", hasher.finalize());
    if downloaded != spec.size {
        let _ = tokio::fs::remove_file(dest).await;
        return Err(format!(
            "安装包大小不符：期望 {} 字节，实际 {}",
            spec.size, downloaded
        ));
    }
    if !digest.eq_ignore_ascii_case(&spec.sha256) {
        let _ = tokio::fs::remove_file(dest).await;
        return Err("安装包 sha256 校验失败，已放弃本次更新".into());
    }
    Ok(())
}

fn temp_installer_path() -> PathBuf {
    // 不把清单里的资产名当路径用；uuid 随机文件名 + 平台扩展名
    let ext = installer_suffix().unwrap_or(".bin");
    std::env::temp_dir().join(format!("hmi-update-{}{ext}", uuid::Uuid::new_v4()))
}

// —— Tauri 命令 ——

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| format!("初始化 HTTP 客户端失败：{e}"))
}

#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub async fn check_for_updates(
    state: tauri::State<'_, PendingUpdateState>,
) -> Result<UpdateCheck, String> {
    let current =
        Version::parse(env!("CARGO_PKG_VERSION")).map_err(|e| format!("当前版本号异常：{e}"))?;
    let client = http_client()?;
    let response = client
        .get(UPDATE_MANIFEST_URL)
        .timeout(Duration::from_secs(30))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("无法访问 GitHub：{e}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        // 还没有任何已发布版本（releases/latest 只解析已发布的 Release）
        return Ok(UpdateCheck {
            status: UpdateStatus::Unpublished,
            current_version: current.to_string(),
            version: None,
            notes: None,
            published_at: None,
            can_install: false,
            download_size: None,
            installer_kind: None,
        });
    }
    if response.status() == reqwest::StatusCode::FORBIDDEN
        || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        return Err("GitHub 限流或拒绝访问，请稍后再试".into());
    }
    if !response.status().is_success() {
        return Err(format!("获取更新清单失败：HTTP {}", response.status()));
    }
    let manifest: UpdateManifest = response
        .json()
        .await
        .map_err(|e| format!("解析更新清单失败：{e}"))?;
    let check = evaluate_manifest(&manifest, &current);
    // 记住本次选中的安装包，download_update 只装这一个。
    if check.can_install {
        if let Some(spec) = installer_platform()
            .and_then(|key| manifest.installers.get(key))
            .and_then(installer_spec)
        {
            state.0.lock().unwrap().replace(PendingUpdate {
                spec,
                path: temp_installer_path(),
            });
        }
    }
    Ok(check)
}

#[tauri::command]
pub async fn download_update(
    state: tauri::State<'_, PendingUpdateState>,
    on_progress: Channel<UpdateProgress>,
) -> Result<(), String> {
    let pending = state
        .0
        .lock()
        .unwrap()
        .clone()
        .ok_or("请先检查更新，再下载")?;
    if !trusted_download(&pending.spec.url) {
        return Err("下载地址不在信任范围内，已拒绝".into());
    }
    let client = http_client()?;
    let spec = pending.spec.clone();
    let dest = pending.path.clone();
    download_installer(&client, &spec, &dest, |downloaded, total| {
        // 窗口已关闭时发送失败可以忽略
        let _ = on_progress.send(UpdateProgress { downloaded, total });
    })
    .await?;
    state
        .0
        .lock()
        .unwrap()
        .replace(PendingUpdate { spec, path: dest });
    Ok(())
}

#[tauri::command]
pub async fn install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, PendingUpdateState>,
) -> Result<(), String> {
    let pending = state.0.lock().unwrap().take().ok_or("请先下载更新")?;
    // 落盘文件再验一遍，防止下载后被篡改
    let bytes = tokio::fs::read(&pending.path)
        .await
        .map_err(|e| format!("读取安装包失败：{e}"))?;
    verify_installer(&bytes, &pending.spec)?;
    launch_installer(&pending.path)?;
    // 给安装程序一点时间完成提权与启动，再退出自己
    std::thread::sleep(std::time::Duration::from_millis(1500));
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn open_update_release() -> Result<(), String> {
    if cfg!(target_os = "windows") {
        std::process::Command::new("explorer")
            .arg(REPO_RELEASES_URL)
            .spawn()
            .map_err(|e| format!("打开发布页失败：{e}"))?;
    } else {
        std::process::Command::new("open")
            .arg(REPO_RELEASES_URL)
            .spawn()
            .map_err(|e| format!("打开发布页失败：{e}"))?;
    }
    Ok(())
}

/// 分离（detached）拉起安装程序：父进程退出不能带走安装程序，
/// 并尝试脱离本进程的 job object（Windows 安装服务约束）。
fn launch_installer(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let detached = CREATE_NEW_PROCESS_GROUP | CREATE_DETACHED_PROCESS;
        let spawn = |flags: u32| {
            std::process::Command::new(path)
                .creation_flags(flags)
                .spawn()
        };
        match spawn(detached | CREATE_BREAKAWAY_FROM_JOB) {
            Ok(_) => Ok(()),
            // 有些环境不允许 breakaway，退回普通分离启动
            Err(e) => match spawn(detached) {
                Ok(_) => Ok(()),
                Err(_) => Err(format!("启动安装程序失败：{e}")),
            },
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("启动安装程序失败：{e}"))
    }
}

// —— 单测 ——

#[cfg(test)]
mod tests {
    use super::*;

    fn installer(name: &str, url: &str, size: u64, sha256: &str) -> ManifestInstaller {
        ManifestInstaller {
            name: name.into(),
            url: url.into(),
            size,
            sha256: sha256.into(),
        }
    }

    fn manifest(version: &str, installers: Vec<(&str, ManifestInstaller)>) -> UpdateManifest {
        UpdateManifest {
            version: version.into(),
            notes: Some("notes".into()),
            published_at: Some("2026-01-01T00:00:00Z".into()),
            installers: installers
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        }
    }

    const GOOD_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    /// 生成端写的是 encodeURIComponent 后的 URL（productName 带空格 → %20）。
    const GOOD_URL: &str =
        "https://github.com/mkktop/plan_M/releases/download/v0.3.0/HMI%20Platform_0.3.0_x64-setup.exe";

    fn current() -> Version {
        Version::new(0, 2, 0)
    }

    #[test]
    fn parses_release_versions() {
        assert_eq!(release_version("v1.2.3"), Some(Version::new(1, 2, 3)));
        assert_eq!(release_version("V10.0.0"), Some(Version::new(10, 0, 0)));
        assert_eq!(release_version("1.2.3"), Some(Version::new(1, 2, 3)));
        assert_eq!(release_version("v1.2"), None);
        assert_eq!(release_version("main"), None);
    }

    #[test]
    fn skips_bad_and_prerelease_versions() {
        // 版本号解析不出来 → 不提示更新
        let m = manifest("bad-tag", vec![]);
        assert_eq!(
            evaluate_manifest(&m, &current()).status,
            UpdateStatus::Current
        );
        // 预发布版本（rc/beta）→ 跳过自动更新提示
        let m = manifest(
            "0.3.0-rc.1",
            vec![(
                "windows-x64",
                installer(
                    "planM_0.3.0-rc.1_x64-setup.exe",
                    GOOD_URL,
                    8_000_000,
                    GOOD_SHA256,
                ),
            )],
        );
        assert_eq!(
            evaluate_manifest(&m, &current()).status,
            UpdateStatus::Current
        );
    }

    #[test]
    fn same_or_lower_version_is_current() {
        let m = manifest("0.2.0", vec![]);
        assert_eq!(
            evaluate_manifest(&m, &current()).status,
            UpdateStatus::Current
        );
        let m = manifest("0.1.9", vec![]);
        assert_eq!(
            evaluate_manifest(&m, &current()).status,
            UpdateStatus::Current
        );
    }

    #[test]
    fn selects_platform_installer_with_digest() {
        if !cfg!(target_os = "windows") {
            // 应用内安装目前只在 Windows 上提供
            return;
        }
        let m = manifest(
            "0.3.0",
            vec![
                (
                    "windows-x64",
                    installer(
                        "planM_0.3.0_x64-setup.exe",
                        GOOD_URL,
                        8_000_000,
                        GOOD_SHA256,
                    ),
                ),
                // 其他平台的清单项按 key 直接忽略
                (
                    "macos-arm64",
                    installer("planM_0.3.0_aarch64.dmg", GOOD_URL, 8_000_000, GOOD_SHA256),
                ),
            ],
        );
        let check = evaluate_manifest(&m, &current());
        assert_eq!(check.status, UpdateStatus::Available);
        assert!(check.can_install);
        assert_eq!(check.download_size, Some(8_000_000));
        assert_eq!(check.version.as_deref(), Some("0.3.0"));

        // 有新版本但资产不可校验（坏摘要 / 超大 / 后缀不符）→ 提示更新但不允许应用内安装
        let m = manifest(
            "0.3.0",
            vec![
                (
                    "windows-x64",
                    installer("planM_0.3.0_x64-setup.exe", GOOD_URL, 100, "nothex"),
                ),
                (
                    "windows-x64-backup",
                    installer("x.bin", GOOD_URL, MAX_INSTALLER_SIZE + 1, GOOD_SHA256),
                ),
            ],
        );
        let check = evaluate_manifest(&m, &current());
        assert_eq!(check.status, UpdateStatus::Available);
        assert!(!check.can_install);
    }

    #[test]
    fn rejects_untrusted_download_urls() {
        assert!(trusted_download(GOOD_URL));
        assert!(!trusted_download(
            "http://github.com/mkktop/plan_M/releases/download/v0.3.0/a.exe"
        ));
        assert!(!trusted_download(
            "https://evil.com/mkktop/plan_M/releases/download/v0.3.0/a.exe"
        ));
        assert!(!trusted_download(
            "https://github.com/other/repo/releases/download/v0.3.0/a.exe"
        ));
        assert!(!trusted_download(
            "https://github.com/mkktop/plan_M/other/a.exe"
        ));
        // 带端口 / 查询串 / 用户信息 / 锚点的一律拒绝
        assert!(!trusted_download(
            "https://github.com:8443/mkktop/plan_M/releases/download/v0.3.0/a.exe"
        ));
        assert!(!trusted_download(
            "https://github.com/mkktop/plan_M/releases/download/v0.3.0/a.exe?x=1"
        ));
        assert!(!trusted_download(
            "https://user:pass@github.com/mkktop/plan_M/releases/download/v0.3.0/a.exe"
        ));
        assert!(!trusted_download(
            "https://github.com/mkktop/plan_M/releases/download/v0.3.0/a.exe#frag"
        ));
    }

    #[test]
    fn verifies_size_and_digest() {
        let spec = InstallerSpec {
            url: "https://github.com/mkktop/plan_M/releases/download/v0.3.0/a.exe".into(),
            sha256: sha256_bytes(b"installer-bytes"),
            size: b"installer-bytes".len() as u64,
        };
        assert!(verify_installer(b"installer-bytes", &spec).is_ok());
        assert!(verify_installer(b"tampered-bytes!", &spec).is_err());
        assert!(verify_installer(b"short", &spec).is_err());
    }

    #[tokio::test]
    async fn downloads_stream_with_hash_verification() {
        // 本地 TCP 服务器当假发布源：下载器本身不限制协议，信任检查在命令层做。
        let payload: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let served = payload.clone();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let mut request = Vec::new();
            loop {
                let n = socket.read(&mut buf).await.unwrap();
                if n == 0 {
                    return;
                }
                request.extend_from_slice(&buf[..n]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                served.len()
            );
            socket.write_all(header.as_bytes()).await.unwrap();
            socket.write_all(&served).await.unwrap();
            socket.shutdown().await.unwrap();
        });

        let spec = InstallerSpec {
            url: format!("http://{addr}/mkktop/plan_M/releases/download/v0.3.0/a.exe"),
            sha256: sha256_bytes(&payload),
            size: payload.len() as u64,
        };
        let dest = std::env::temp_dir().join(format!("hmi-test-update-{}.bin", std::process::id()));
        let client = reqwest::Client::new();
        let mut last = (0u64, 0u64);
        download_installer(&client, &spec, &dest, |downloaded, total| {
            last = (downloaded, total);
        })
        .await
        .expect("download should succeed");
        server.abort();
        assert_eq!(last.0, spec.size);
        assert_eq!(last.1, spec.size);
        assert_eq!(tokio::fs::read(&dest).await.unwrap(), payload);
        let _ = tokio::fs::remove_file(&dest).await;

        // 摘要被篡改时必须失败且不落盘
        let mut bad = spec.clone();
        bad.sha256 = "0".repeat(64);
        download_installer(&client, &bad, &dest, |_, _| {})
            .await
            .unwrap_err();
        assert!(!dest.exists());
    }
}
