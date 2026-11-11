"""把 CI 构建出的安装包连同来源清单（sha256 / git 树哈希）staging 成工件。

环境变量：
  INSTALLER_PLATFORM  windows-x64
  INSTALLER_GLOB      target/release/bundle/nsis/*.exe
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

platform = os.environ["INSTALLER_PLATFORM"]
assert platform == "windows-x64", f"unsupported platform: {platform}"
files = list(Path(".").glob(os.environ["INSTALLER_GLOB"]))
assert len(files) == 1, f"expected exactly one installer, got {files}"
source = files[0]
version = json.loads(Path("apps/desktop/src-tauri/tauri.conf.json").read_text())["version"]
suffix = "_x64-setup.exe"
assert source.name.endswith(f"_{version}{suffix}"), (
    f"installer name {source.name!r} does not end with _{version}{suffix}"
)
directory = Path("artifacts/installers") / platform
directory.mkdir(parents=True, exist_ok=True)
shutil.copyfile(source, directory / source.name)
manifest = {
    "platform": platform,
    "version": version,
    "filename": source.name,
    "size": source.stat().st_size,
    "sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
    "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
    "source_tree": subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"], text=True).strip(),
}
(directory / "build-info.json").write_text(json.dumps(manifest, indent=2) + "\n")
print(json.dumps(manifest))
