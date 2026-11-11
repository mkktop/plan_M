//! 设备包（Device Package）：声明式描述"某型号设备怎么通信、有哪些点"。
//!
//! - [`manifest`]：manifest.json 模型
//! - [`validate`]：语义校验（errors 拒绝 / warnings 提示）
//! - [`install`]：安装（目录或 zip → 校验 → 落盘）与注册表（目录为真相源）
//!
//! 安装后的布局：`<data_dir>/packages/<包id>/<版本>/{manifest.json, points.json, ...}`

pub mod install;
pub mod manifest;
pub mod validate;

pub use install::{
    install, load_package_dir, InstalledPackage, LoadedPackage, PackageError, PackageRegistry,
};
pub use manifest::{PackageManifest, ProtocolDecl};
pub use validate::{validate_package, ValidationIssues};

/// JSON Schema 文本（供编辑器补全/校验，与运行时 Rust 校验并行存在）。
pub mod schemas {
    pub const MANIFEST: &str = include_str!("../schema/manifest.schema.json");
    pub const POINTS: &str = include_str!("../schema/points.schema.json");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_embedded() {
        assert!(schemas::MANIFEST.contains("schemaVersion"));
        assert!(schemas::POINTS.contains("pollGroups"));
    }
}
