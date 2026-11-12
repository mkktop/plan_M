//! 设备包安装与注册表。
//!
//! 安装流：源（目录或 zip）→ 物化到临时目录 → 解析校验 → 宿主版本检查
//! → 原子移入 `<root>/<包id>/<版本>/`。
//! 注册表以目录为真相源：启动时扫描重建，目录被手动删除也能自愈。

use crate::manifest::PackageManifest;
use crate::validate::validate_package;
use hmi_domain::driver::Driver;
use hmi_domain::points::PointsTable;
use semver::Version;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PackageError {
    #[error("I/O 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("{file} 解析失败（行 {line} 列 {col}）：{msg}")]
    Parse {
        file: String,
        line: usize,
        col: usize,
        msg: String,
    },
    #[error("缺少文件：{0}")]
    MissingFile(String),
    #[error("包校验未通过：\n  - {0}")]
    Invalid(String),
    #[error("需要宿主版本 >= {need}，当前 {have}")]
    IncompatibleHost { need: Version, have: Version },
    #[error("不支持的包源：{0}（需要目录或 .zip）")]
    UnsupportedSource(String),
    #[error("zip 条目路径不安全：{0}")]
    UnsafeZipEntry(String),
}

/// 解析并校验后的设备包（内存态）。
#[derive(Clone, Debug)]
pub struct LoadedPackage {
    pub manifest: PackageManifest,
    pub points: PointsTable,
}

/// 从目录读取并完整校验一个设备包（manifest.json + points.json）。
pub fn load_package_dir(
    dir: &Path,
    drivers: &[Arc<dyn Driver>],
) -> Result<LoadedPackage, PackageError> {
    let manifest_path = dir.join("manifest.json");
    if !manifest_path.is_file() {
        return Err(PackageError::MissingFile("manifest.json".into()));
    }
    let points_path = dir.join("points.json");
    if !points_path.is_file() {
        return Err(PackageError::MissingFile("points.json".into()));
    }

    let mut manifest: PackageManifest = parse_json_file(&manifest_path)?;
    // v2 页面 / 包内组件短名归一化（安装校验与运行时 DTO 共用同一形态）
    manifest.ui.normalize(&manifest.id);
    let points: PointsTable = parse_json_file(&points_path)?;

    let mut issues = validate_package(&manifest, &points, drivers);
    // 组件入口文件存在性（校验函数拿不到目录，这里补）
    for w in &manifest.ui.widgets {
        if !dir.join(&w.entry).is_file() {
            issues.errors.push(format!(
                "ui.widgets `{}`：entry 文件不存在：{}",
                w.id, w.entry
            ));
        }
    }
    if !issues.ok() {
        return Err(PackageError::Invalid(issues.errors.join("\n  - ")));
    }
    Ok(LoadedPackage { manifest, points })
}

fn parse_json_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, PackageError> {
    let text = fs::read_to_string(path)?;
    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "?".into());
    serde_json::from_str(&text).map_err(|e| PackageError::Parse {
        file,
        line: e.line(),
        col: e.column(),
        msg: e.to_string(),
    })
}

/// 安装设备包到 `dest_root`（通常是 `<data_dir>/packages`）。
/// 返回安装后的落盘信息。
pub fn install(
    src: &Path,
    dest_root: &Path,
    host_version: &Version,
    drivers: &[Arc<dyn Driver>],
) -> Result<InstalledPackage, PackageError> {
    // 1. 物化源到临时目录（时间戳+进程id+序号保证唯一）
    let work_dir = unique_temp_dir("hmi-pkg-install");

    let result = (|| -> Result<InstalledPackage, PackageError> {
        materialize(src, &work_dir)?;
        // zip 解压后可能内嵌一层目录，取"包含 manifest.json 的目录"
        let pkg_dir = locate_pkg_dir(&work_dir)?;

        // 2. 解析 + 校验
        let loaded = load_package_dir(&pkg_dir, drivers)?;

        // 3. 宿主版本检查
        if let Some(need) = &loaded.manifest.min_host_version {
            if host_version < need {
                return Err(PackageError::IncompatibleHost {
                    need: need.clone(),
                    have: host_version.clone(),
                });
            }
        }

        // 4. 落盘：dest_root/<id>/<version>/（幂等：已存在先删）
        let target = dest_root
            .join(&loaded.manifest.id)
            .join(loaded.manifest.version.to_string());
        if target.exists() {
            fs::remove_dir_all(&target)?;
        }
        fs::create_dir_all(&target)?;
        copy_dir_recursive(&pkg_dir, &target)?;

        Ok(InstalledPackage {
            manifest: loaded.manifest,
            points: loaded.points,
            dir: target,
        })
    })();

    let _ = fs::remove_dir_all(&work_dir);
    result
}

pub(crate) fn unique_temp_dir(prefix: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{t}-{}-{n}", std::process::id()))
}

/// 把源（目录或 zip）复制/解压到 dst。
pub(crate) fn materialize(src: &Path, dst: &Path) -> Result<(), PackageError> {
    if src.is_dir() {
        fs::create_dir_all(dst)?;
        copy_dir_recursive(src, dst)?;
        Ok(())
    } else if src
        .extension()
        .map(|e| e.eq_ignore_ascii_case("zip"))
        .unwrap_or(false)
    {
        unzip(src, dst)
    } else {
        Err(PackageError::UnsupportedSource(
            src.to_string_lossy().to_string(),
        ))
    }
}

fn unzip(zip_path: &Path, dst: &Path) -> Result<(), PackageError> {
    fs::create_dir_all(dst)?;
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| PackageError::Invalid(format!("打开 zip 失败: {e}")))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| PackageError::Invalid(format!("读取 zip 条目失败: {e}")))?;
        // zip-slip 防护：拒绝绝对路径与 ..
        let name = entry.name().to_string();
        let path = Path::new(&name);
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(PackageError::UnsafeZipEntry(name));
        }
        let out_path = dst.join(path);
        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }
    Ok(())
}

/// zip 常见"单层包裹"：优先取 work_dir 本身，否则找子目录里的 manifest.json。
pub(crate) fn locate_pkg_dir(work_dir: &Path) -> Result<PathBuf, PackageError> {
    if work_dir.join("manifest.json").is_file() {
        return Ok(work_dir.to_path_buf());
    }
    let mut stack = vec![work_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if p.join("manifest.json").is_file() {
                        return Ok(p);
                    }
                    stack.push(p);
                }
            }
        }
    }
    Err(PackageError::MissingFile(
        "解压/复制后找不到 manifest.json".into(),
    ))
}

pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// 已安装的设备包（磁盘态）。
#[derive(Clone, Debug)]
pub struct InstalledPackage {
    pub manifest: PackageManifest,
    pub points: PointsTable,
    pub dir: PathBuf,
}

/// 包注册表：`root/<包id>/<版本>/`，每个 id 取最高版本。
#[derive(Debug, Default)]
pub struct PackageRegistry {
    packages: Vec<InstalledPackage>,
    /// 扫描时跳过的损坏包（路径, 原因）
    pub broken: Vec<(PathBuf, String)>,
}

impl PackageRegistry {
    /// 扫描目录重建注册表。根目录不存在视为空（不报错）。
    pub fn scan(root: &Path, drivers: &[Arc<dyn Driver>]) -> Self {
        let mut reg = Self::default();
        let Ok(ids) = fs::read_dir(root) else {
            return reg;
        };
        for id_dir in ids.flatten() {
            if !id_dir.path().is_dir() {
                continue;
            }
            let Ok(versions) = fs::read_dir(id_dir.path()) else {
                continue;
            };
            // 每个版本目录都尝试加载，同一 id 留最高版本
            let mut candidates: Vec<InstalledPackage> = Vec::new();
            for ver_dir in versions.flatten() {
                let p = ver_dir.path();
                if !p.is_dir() {
                    continue;
                }
                match load_package_dir(&p, drivers) {
                    Ok(loaded) => candidates.push(InstalledPackage {
                        manifest: loaded.manifest,
                        points: loaded.points,
                        dir: p,
                    }),
                    Err(e) => reg.broken.push((p, e.to_string())),
                }
            }
            if candidates.is_empty() {
                continue;
            }
            candidates.sort_by(|a, b| b.manifest.version.cmp(&a.manifest.version));
            if let Some(best) = candidates.into_iter().next() {
                reg.packages.push(best);
            }
        }
        reg
    }

    pub fn get(&self, id: &str) -> Option<&InstalledPackage> {
        self.packages.iter().find(|p| p.manifest.id == id)
    }

    pub fn list(&self) -> &[InstalledPackage] {
        &self.packages
    }

    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_drivers() -> Vec<Arc<dyn Driver>> {
        vec![Arc::new(hmi_driver_modbus::ModbusDriver::new())]
    }

    fn write_pkg(dir: &Path, id: &str, version: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("manifest.json"),
            serde_json::json!({
                "schemaVersion": 2,
                "id": id,
                "name": "测试包",
                "version": version,
                "protocol": { "driver": "modbus" }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            dir.join("points.json"),
            serde_json::json!({
                "pollGroups": [{ "name": "fast", "intervalMs": 1000 }],
                "points": [
                    { "id": "voltage", "name": "电压", "address": "40101",
                      "type": "u16", "scale": 0.1, "unit": "V", "pollGroup": "fast" }
                ]
            })
            .to_string(),
        )
        .unwrap();
    }

    #[test]
    fn install_from_dir_and_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let root = tmp.path().join("pkgs");
        write_pkg(&src, "com.test.meter", "1.0.0");

        let host = Version::new(0, 1, 0);
        let installed = install(&src, &root, &host, &test_drivers()).unwrap();
        assert!(installed.dir.join("manifest.json").is_file());
        assert_eq!(installed.manifest.id, "com.test.meter");

        let reg = PackageRegistry::scan(&root, &test_drivers());
        assert_eq!(reg.list().len(), 1);
        let p = reg.get("com.test.meter").unwrap();
        assert_eq!(p.points.points.len(), 1);
    }

    #[test]
    fn upgrade_keeps_latest() {
        let tmp = tempfile::tempdir().unwrap();
        let src_v1 = tmp.path().join("v1");
        let src_v2 = tmp.path().join("v2");
        let root = tmp.path().join("pkgs");
        write_pkg(&src_v1, "com.test.meter", "1.0.0");
        write_pkg(&src_v2, "com.test.meter", "1.1.0");

        let host = Version::new(0, 1, 0);
        install(&src_v1, &root, &host, &test_drivers()).unwrap();
        install(&src_v2, &root, &host, &test_drivers()).unwrap();

        let reg = PackageRegistry::scan(&root, &test_drivers());
        assert_eq!(reg.list().len(), 1); // 同 id 一个条目
        assert_eq!(
            reg.get("com.test.meter")
                .unwrap()
                .manifest
                .version
                .to_string(),
            "1.1.0"
        );
    }

    #[test]
    fn incompatible_host_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        write_pkg(&src, "com.test.meter", "1.0.0");
        // 手工加最低宿主要求
        let m = fs::read_to_string(src.join("manifest.json")).unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&m).unwrap();
        v["minHostVersion"] = serde_json::json!("9.0.0");
        fs::write(src.join("manifest.json"), v.to_string()).unwrap();

        let r = install(
            &src,
            &tmp.path().join("pkgs"),
            &Version::new(0, 1, 0),
            &test_drivers(),
        );
        assert!(matches!(r, Err(PackageError::IncompatibleHost { .. })));
    }

    #[test]
    fn broken_package_reported_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("pkgs");
        let bad = root.join("com.test.bad").join("1.0.0");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join("manifest.json"), "{ not json").unwrap();

        let reg = PackageRegistry::scan(&root, &test_drivers());
        assert!(reg.is_empty());
        assert_eq!(reg.broken.len(), 1);
    }

    #[test]
    fn missing_points_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(
            src.join("manifest.json"),
            r#"{ "schemaVersion":2, "id":"a.b", "name":"x", "version":"1.0.0",
                 "protocol": { "driver": "modbus" } }"#,
        )
        .unwrap();
        assert!(matches!(
            load_package_dir(&src, &test_drivers()),
            Err(PackageError::MissingFile(_))
        ));
    }
}
