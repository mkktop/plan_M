#!/usr/bin/env node
// 一次性同步 4 处版本号：根 Cargo.toml（workspace）、tauri.conf.json、两个 package.json。
// 用法：node scripts/set-version.mjs 0.2.0
import { readFileSync, writeFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const arg = process.argv[2]
if (!/^\d+\.\d+\.\d+$/.test(arg || '')) {
  console.error('用法：node scripts/set-version.mjs X.Y.Z（不带 v 前缀）')
  process.exit(1)
}
const version = arg

const files = [
  {
    path: 'Cargo.toml',
    // 只替换 [workspace.package] 段的行首 version（依赖区的 version 都在行内表格里，不会命中）
    replace: (s) => s.replace(/^(\[workspace\.package\][\s\S]*?^version = )"[\d.]+"$/m, `$1"${version}"`),
    read: () => {
      const m = readFileSync(path.join(root, 'Cargo.toml'), 'utf8').match(/^version = "([\d.]+)"$/m)
      return m?.[1]
    },
  },
  {
    path: 'apps/desktop/src-tauri/tauri.conf.json',
    json: 'version',
  },
  {
    path: 'apps/desktop/package.json',
    json: 'version',
  },
  {
    path: 'apps/desktop/ui/package.json',
    json: 'version',
  },
]

for (const file of files) {
  const full = path.join(root, file.path)
  if (file.json) {
    const data = JSON.parse(readFileSync(full, 'utf8'))
    data.version = version
    writeFileSync(full, JSON.stringify(data, null, 2) + '\n')
  } else {
    const before = readFileSync(full, 'utf8')
    const after = file.replace(before)
    if (after === before) {
      console.error(`未能替换 ${file.path} 的版本号（正则未命中）`)
      process.exit(1)
    }
    writeFileSync(full, after)
  }
  console.log(`${file.path} → ${version}`)
}

// 尽力刷新 Cargo.lock 里的成员版本；cargo 不在 PATH 时提醒手动执行。
const cargo = spawnSync('cargo', ['update', '--workspace', '--quiet'], {
  cwd: root,
  shell: process.platform === 'win32',
})
if (cargo.status === 0) {
  console.log('Cargo.lock 已刷新')
} else {
  console.warn('提示：请手动执行 `cargo update --workspace` 刷新 Cargo.lock')
}
