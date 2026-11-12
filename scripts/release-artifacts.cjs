// Release 工作流用的校验/上传逻辑（actions/github-script 直接 require）。
// 设计与 pika_client 的同名脚本一致：只把「在 main 分支上构建并验证过的安装包」提升为 draft Release。
const fs = require('node:fs')
const path = require('node:path')
const crypto = require('node:crypto')
const { execFileSync } = require('node:child_process')

const WORKFLOW_FILE = 'check.yml'
const PLATFORM_SUFFIXES = { 'windows-x64': '_x64-setup.exe' }

function tree(ref) {
  return execFileSync('git', ['rev-parse', `${ref}^{tree}`], { encoding: 'utf8' }).trim()
}

function validRun(run, context) {
  return (
    run.status === 'completed' &&
    run.conclusion === 'success' &&
    run.path === `.github/workflows/${WORKFLOW_FILE}` &&
    run.head_repository.full_name === `${context.repo.owner}/${context.repo.repo}` &&
    /^[0-9a-f]{40}$/.test(run.head_sha)
  )
}

/** 找到与当前 checkout 源码树一致的成功构建 run（tag 必须先在 master 上过 CI）。 */
async function findVerifiedRun({ github, context, tag }) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag)) throw new Error(`Invalid release tag: ${tag}`)
  execFileSync('node', ['scripts/check-release-version.mjs', tag], { stdio: 'inherit' })
  const { data } = await github.rest.actions.listWorkflowRuns({
    ...context.repo,
    workflow_id: WORKFLOW_FILE,
    status: 'success',
    per_page: 50,
  })
  const expected = tree('HEAD')
  for (const run of data.workflow_runs) {
    if (!validRun(run, context)) continue
    try {
      if (tree(run.head_sha) === expected) return String(run.id)
    } catch {
      /* 无关工作流提交可能不在本 tag 历史里，跳过 */
    }
  }
  throw new Error('没有与该 tag 源码一致的成功构建；先让 check.yml 跑绿再重试')
}

/** 校验每个工件的来源/版本/摘要后再写 Release；拒绝改已发布的 Release。 */
async function uploadInstallers({ github, context, core, tag, directory, platforms }) {
  const version = tag.slice(1)
  const expectedTree = tree('HEAD')
  const installers = platforms.map((platform) => {
    if (!PLATFORM_SUFFIXES[platform]) throw new Error(`Unsupported platform: ${platform}`)
    const folder = path.join(directory, `${platform}-installer`)
    const manifest = JSON.parse(fs.readFileSync(path.join(folder, 'build-info.json'), 'utf8'))
    if (
      manifest.platform !== platform ||
      manifest.version !== version ||
      manifest.source_tree !== expectedTree ||
      path.basename(manifest.filename) !== manifest.filename ||
      !manifest.filename.endsWith(`_${version}${PLATFORM_SUFFIXES[platform]}`)
    ) {
      throw new Error(`Installer provenance or version mismatch: ${platform}`)
    }
    const data = fs.readFileSync(path.join(folder, manifest.filename))
    const digest = `sha256:${crypto.createHash('sha256').update(data).digest('hex')}`
    if (
      data.length !== manifest.size ||
      digest !== `sha256:${manifest.sha256}` ||
      data.subarray(0, 2).toString() !== 'MZ'
    ) {
      throw new Error(`Installer checksum or format mismatch: ${platform}`)
    }
    return {
      platform,
      name: manifest.filename,
      uploadedName: null,
      suffix: PLATFORM_SUFFIXES[platform],
      data,
      digest,
    }
  })
  const { data: releases } = await github.rest.repos.listReleases({
    ...context.repo,
    per_page: 100,
  })
  let release = releases.find((item) => item.tag_name === tag)
  if (release && !release.draft) throw new Error('Refusing to modify a published release')
  if (!release) {
    ;({ data: release } = await github.rest.repos.createRelease({
      ...context.repo,
      tag_name: tag,
      name: `planM ${tag}`,
      draft: true,
      prerelease: false,
      body: [
        '上位机平台桌面版安装包（Windows x64）。',
        '',
        '- 安装包内附带同版本 `gatewayd.exe` 采集服务（安装目录下，手动运行 `gatewayd serve --sim` 或真实串口配置；数据目录 `%APPDATA%/gatewayd`）。',
        '- 应用内「设置 → 检查更新」可自动完成后续升级。',
        '- 安装包未做代码签名，Windows SmartScreen 可能提示「更多信息 → 仍要运行」。',
      ].join('\n'),
    }))
  }
  for (const installer of installers) {
    const existing = release.assets.filter((asset) =>
      asset.name.endsWith(`_${version}${installer.suffix}`),
    )
    if (existing.length) {
      if (
        existing.length !== 1 ||
        existing[0].digest !== installer.digest ||
        existing[0].size !== installer.data.length
      ) {
        throw new Error('Release draft contains a different installer')
      }
      // 复用已有资产时以 GitHub 报告的（可能去重改名后的）规范名为准
      installer.uploadedName = existing[0].name
    } else {
      const { data: upload } = await github.rest.repos.uploadReleaseAsset({
        ...context.repo,
        release_id: release.id,
        name: installer.name,
        data: installer.data,
        headers: { 'content-type': 'application/octet-stream' },
      })
      installer.uploadedName = upload.data?.name ?? upload.name ?? installer.name
    }
    core.info(`${installer.uploadedName}: ${installer.data.length} bytes, ${installer.digest}`)
  }

  // 应用内更新读 releases/latest/download/latest.json（不走 api.github.com，避开限流）。
  // 文件名带空格，URL 必须百分号编码；摘要去掉 sha256: 前缀存纯 hex。
  const updateManifest = {
    version,
    notes: release.body || '',
    published_at: new Date().toISOString(),
    installers: Object.fromEntries(
      installers.map((installer) => [
        installer.platform,
        {
          name: installer.uploadedName,
          url: `https://github.com/${context.repo.owner}/${context.repo.repo}/releases/download/${tag}/${encodeURIComponent(installer.uploadedName)}`,
          size: installer.data.length,
          sha256: installer.digest.slice('sha256:'.length),
        },
      ]),
    ),
  }
  // 先删旧的再传：latest.json 需要跟 releases/latest/download/latest.json 对齐。
  // 注意 CDN 对该地址有缓存，发布后客户端最长可能看到约 1 小时的旧清单。
  for (const asset of release.assets.filter((item) => item.name === 'latest.json')) {
    await github.rest.repos.deleteReleaseAsset({ ...context.repo, asset_id: asset.id })
  }
  await github.rest.repos.uploadReleaseAsset({
    ...context.repo,
    release_id: release.id,
    name: 'latest.json',
    data: Buffer.from(JSON.stringify(updateManifest, null, 2) + '\n'),
    headers: { 'content-type': 'application/json' },
  })
  core.info(`Update manifest latest.json uploaded (${Object.keys(updateManifest.installers).join(', ')})`)
  core.info(`Verified installers uploaded to Release draft ${release.id}`)
}

module.exports = { findVerifiedRun, uploadInstallers }
