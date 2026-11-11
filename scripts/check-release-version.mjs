#!/usr/bin/env node
// 发版前校验：tag 必须是 v语义化版本，且与 4 处版本声明完全一致。
// 用法：node scripts/check-release-version.mjs v1.2.3
import { readFileSync } from 'node:fs'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const tag = process.argv[2]
assert.match(
  tag || '',
  /^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/,
  'Release 标签需要使用 v版本号，如 v1.2.3',
)
const version = tag.slice(1)

const json = (rel) =>
  JSON.parse(readFileSync(new URL('../' + rel, import.meta.url), 'utf8'))
assert.equal(
  json('apps/desktop/src-tauri/tauri.conf.json').version,
  version,
  'tauri.conf.json 版本号不一致',
)
assert.equal(json('apps/desktop/package.json').version, version, 'apps/desktop/package.json 版本号不一致')
assert.equal(json('apps/desktop/ui/package.json').version, version, 'ui/package.json 版本号不一致')

const cargo = readFileSync(new URL('../Cargo.toml', import.meta.url), 'utf8')
assert.equal(
  cargo.match(/^(\[workspace\.package\][\s\S]*?^version = )"([\d.]+)"$/m)?.[2],
  version,
  '根 Cargo.toml workspace 版本号不一致',
)

console.log(`版本号一致性检查通过：${tag}`)
