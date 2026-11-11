//! pkg 子命令：设备包 validate / install。

use anyhow::{bail, Result};
use semver::Version;

pub enum PkgCmd {
    Validate {
        path: std::path::PathBuf,
    },
    Install {
        path: std::path::PathBuf,
        data_dir: Option<std::path::PathBuf>,
    },
}

pub async fn run(cmd: PkgCmd) -> Result<()> {
    match cmd {
        PkgCmd::Validate { path } => validate(&path),
        PkgCmd::Install { path, data_dir } => install(&path, data_dir).await,
    }
}

fn validate(path: &std::path::Path) -> Result<()> {
    println!("校验设备包: {}", path.display());
    match hmi_packaging::load_package_dir(path) {
        Ok(pkg) => {
            println!("  ✔ manifest 解析通过");
            println!(
                "  ✔ id={}  name={}  version={}  driver={}",
                pkg.manifest.id,
                pkg.manifest.name,
                pkg.manifest.version,
                pkg.manifest.protocol.driver
            );
            // 重新跑一次带 warnings 的完整校验
            let issues = hmi_packaging::validate_package(&pkg.manifest, &pkg.points);
            println!("  ✔ 点表解析通过（{} 点）", pkg.points.points.len());
            for p in pkg.points.points.iter().take(5) {
                println!(
                    "    · {} @ {} [{}] {}",
                    p.id,
                    p.register.to_engineering(),
                    p.data_type.as_str(),
                    p.name
                );
            }
            if pkg.points.points.len() > 5 {
                println!("    · … 共 {} 点", pkg.points.points.len());
            }
            for w in &issues.warnings {
                println!("  ⚠ {w}");
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("  ✘ 校验失败:\n{e}");
            bail!("设备包校验未通过");
        }
    }
}

async fn install(path: &std::path::Path, data_dir: Option<std::path::PathBuf>) -> Result<()> {
    let root = data_dir
        .unwrap_or_else(crate::default_data_dir)
        .join("packages");
    let host = Version::parse(crate::HOST_VERSION)?;
    match hmi_packaging::install(path, &root, &host) {
        Ok(installed) => {
            println!(
                "  ✔ 已安装 {} v{} → {}",
                installed.manifest.id,
                installed.manifest.version,
                installed.dir.display()
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("  ✘ 安装失败:\n{e}");
            bail!("设备包安装失败");
        }
    }
}
