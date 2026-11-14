// 原生安装、版本和产物校验共用入口；只有独立发布命令持有 GitHub 写入权限。
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
export const platforms = {
  'windows-x64': { suffix: 'windows_x64-setup.exe', os: 'windows', arch: 'x86_64' },
  'linux-x64': { suffix: 'linux_amd64.deb', os: 'linux', arch: 'x86_64' },
  'macos-arm64': { suffix: 'macos_arm64.dmg', os: 'macos', arch: 'aarch64' },
  'macos-x64': { suffix: 'macos_x86_64.dmg', os: 'macos', arch: 'x86_64' },
};

// 本次正式发行覆盖 Windows 和两种 Mac；Linux 可作为额外验证产物发布。
export const releasePlatforms = ['windows-x64', 'macos-arm64', 'macos-x64'];

/** 执行原生命令并保留失败输出；不通过 shell 拼接文件名或用户输入。 */
function run(program, args, options = {}) {
  const result = spawnSync(program, args, {
    cwd: root,
    encoding: 'utf8',
    timeout: 180_000,
    maxBuffer: 16 * 1024 * 1024,
    ...options,
  });
  assert.ifError(result.error);
  assert.equal(
    result.status,
    0,
    `${program} ${args.join(' ')}\n${result.stdout}\n${result.stderr}`,
  );
  return result.stdout.trim();
}

/** 前端、锁文件、Rust 和 Tauri 必须使用同一个正式版本与稳定应用标识。 */
export function metadata(base = root) {
  const json = (name) => JSON.parse(fs.readFileSync(path.join(base, name), 'utf8'));
  const config = json('src-tauri/tauri.conf.json');
  const pkg = json('front/package.json');
  const lock = json('front/package-lock.json');
  const cargo = fs.readFileSync(path.join(base, 'src-tauri/Cargo.toml'), 'utf8');
  const cargoLock = fs.readFileSync(path.join(base, 'Cargo.lock'), 'utf8');
  const version = config.version;
  assert.match(version, /^\d+\.\d+\.\d+$/);
  for (const other of [
    pkg.version,
    lock.version,
    lock.packages[''].version,
    cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1],
    cargoLock.match(/name = "lingjing-desktop"\nversion = "([^"]+)"/)?.[1],
  ]) {
    assert.equal(other, version, '前后端版本不一致');
  }
  assert.equal(config.identifier, 'com.explaincraft.lingjing');
  assert.equal(config.productName, 'Lingjing');
  assert.equal(config.mainBinaryName, 'lingjing');
  return {
    version,
    tag: `v${version}`,
    identifier: config.identifier,
    productName: config.productName,
  };
}

/** 只接受单个符合当前平台的安装包，避免把旧版本混入新发布。 */
function installer(platform, meta) {
  assert.ok(platforms[platform], `未知平台 ${platform}`);
  if (platform.startsWith('macos-')) {
    const file = path.join(
      root,
      'dist-release',
      `ExplainCraft_${meta.version}_${platforms[platform].suffix}`,
    );
    assert.ok(fs.existsSync(file), `找不到 ${file}`);
    return file;
  }
  const extension = platform === 'linux-x64' ? '.deb' : '-setup.exe';
  const candidates = fs
    .readdirSync(path.join(root, 'target/release/bundle'), { recursive: true })
    .filter((name) => name.endsWith(extension))
    .map((name) => path.join(root, 'target/release/bundle', name));
  assert.equal(candidates.length, 1, `需要唯一的 ${extension} 安装包`);
  assert.ok(path.basename(candidates[0]).includes(meta.version), '安装包版本不一致');
  return candidates[0];
}

/** 自检只保留系统搜索路径，防止 CI 的 FFmpeg SDK 或 Homebrew 掩盖漏打包的依赖。 */
export function packageEnvironment(platform, executable, base = process.env) {
  const env = { ...base };
  const excluded = new Set([
    'path',
    'ffmpeg_path',
    'ffprobe_path',
    'dyld_library_path',
    'dyld_fallback_library_path',
    'ld_library_path',
  ]);
  for (const key of Object.keys(env)) if (excluded.has(key.toLowerCase())) delete env[key];
  if (platform === 'windows-x64') {
    const system = base.SystemRoot ?? base.SYSTEMROOT ?? 'C:\\Windows';
    env.PATH = [path.win32.dirname(executable), path.win32.join(system, 'System32'), system].join(
      ';',
    );
  } else {
    env.PATH = '/usr/bin:/bin';
  }
  return env;
}

/** 安装或挂载真实安装包，再运行包内程序，验证动态链接、资源和真实媒体编码。 */
function verifyPackage(platform, meta) {
  const file = installer(platform, meta);
  // macOS 的 /var 是符号链接；Tauri 拒绝从此类启动路径加载资源，挂载点必须先规范化。
  const scratch = fs.realpathSync(
    fs.mkdtempSync(path.join(os.tmpdir(), 'explaincraft-install-check-')),
  );
  const report = path.join(scratch, 'self-check.json');
  let mounted = false;
  try {
    let executable;
    if (platform.startsWith('macos-')) {
      const mount = path.join(scratch, 'volume');
      fs.mkdirSync(mount);
      run('hdiutil', ['attach', '-nobrowse', '-readonly', '-mountpoint', mount, file]);
      mounted = true;
      const app = path.join(mount, 'Lingjing.app');
      run('codesign', ['--verify', '--deep', '--strict', app]);
      const plist = path.join(app, 'Contents/Info.plist');
      assert.equal(
        run('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleIdentifier', plist]),
        meta.identifier,
      );
      assert.equal(
        run('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleShortVersionString', plist]),
        meta.version,
      );
      assert.equal(
        run('/usr/libexec/PlistBuddy', ['-c', 'Print :LSMinimumSystemVersion', plist]),
        '15.0',
      );
      executable = path.join(app, 'Contents/MacOS/lingjing');
      assert.equal(
        run('lipo', ['-archs', executable]),
        platform === 'macos-arm64' ? 'arm64' : 'x86_64',
      );
    } else if (platform === 'linux-x64') {
      assert.equal(run('dpkg-deb', ['-f', file, 'Version']), meta.version);
      assert.equal(run('dpkg-deb', ['-f', file, 'Architecture']), 'amd64');
      run('sudo', ['apt-get', 'install', '-y', file]);
      executable = '/usr/bin/lingjing';
      assert.ok(!run('ldd', [executable]).includes('not found'), '安装后的程序缺少动态库');
    } else {
      const destination = path.join(scratch, 'installed');
      run(file, ['/S', `/D=${destination}`]);
      executable = path.join(destination, 'lingjing.exe');
      assert.ok(fs.existsSync(executable), 'NSIS 安装后没有主程序');
    }
    const env = packageEnvironment(platform, executable);
    run(executable, ['--self-check', report], { env });
    const result = JSON.parse(fs.readFileSync(report, 'utf8'));
    for (const key of ['version', 'identifier', 'productName'])
      assert.equal(result[key], meta[key]);
    assert.equal(result.status, 'passed');
    assert.equal(result.os, platforms[platform].os);
    assert.equal(result.arch, platforms[platform].arch);
    assert.equal(result.media, 'h264+aac');
    fs.mkdirSync(path.join(root, 'dist-release'), { recursive: true });
    fs.writeFileSync(
      path.join(root, 'dist-release', `SELF-CHECK-${platform}.json`),
      JSON.stringify(result, null, 2) + '\n',
    );
  } finally {
    if (mounted) run('hdiutil', ['detach', path.join(scratch, 'volume')]);
    fs.rmSync(scratch, { recursive: true, force: true });
  }
}

/** 文件摘要绑定安装包、许可和自检证据，发布时重新计算并拒绝篡改。 */
export function digest(file) {
  return createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

/** 仅在原生测试与安装自检成功之后收集，名称统一使用应用版本和平台。 */
function collect(platform, meta) {
  const directory = path.join(root, 'dist-release');
  const name = `ExplainCraft_${meta.version}_${platforms[platform].suffix}`;
  const source = installer(platform, meta);
  const destination = path.join(directory, name);
  if (source !== destination) fs.copyFileSync(source, destination);
  const license = `LICENSE-${platform}.txt`;
  fs.copyFileSync(path.join(root, 'LICENSE'), path.join(directory, license));
  const check = `SELF-CHECK-${platform}.json`;
  assert.equal(JSON.parse(fs.readFileSync(path.join(directory, check), 'utf8')).status, 'passed');
  const files = [name, license, check].map((name) => ({
    name,
    sha256: digest(path.join(directory, name)),
  }));
  const manifest = {
    schema: 1,
    ...meta,
    platform,
    commit: run('git', ['rev-parse', 'HEAD']),
    runId: process.env.GITHUB_RUN_ID ?? 'local',
    validation: [
      'rustfmt',
      'cargo-test-workspace',
      'frontend-build',
      'installed-package-self-check',
    ],
    files,
  };
  fs.writeFileSync(
    path.join(directory, `BUILD-${platform}.json`),
    JSON.stringify(manifest, null, 2) + '\n',
  );
  fs.writeFileSync(
    path.join(directory, `SHA256SUMS-${platform}.txt`),
    files.map((f) => `${f.sha256}  ${f.name}\n`).join(''),
  );
}

/** 发布前重新验证来源、版本、平台覆盖和所有摘要；部分平台只能发布为预览。 */
export function validateArtifacts({ artifactsDir, outputDir, meta, commit, runId, channel }) {
  assert.ok(['preview', 'stable'].includes(channel));
  assert.match(commit, /^[a-f0-9]{40}$/);
  const manifests = fs
    .readdirSync(artifactsDir, { recursive: true })
    .filter((name) => /^BUILD-.*\.json$/.test(path.basename(name)));
  assert.ok(manifests.length > 0, '没有已验证的安装包');
  const selected = new Set();
  const copies = new Map();
  for (const name of manifests) {
    const file = path.join(artifactsDir, name);
    const manifest = JSON.parse(fs.readFileSync(file, 'utf8'));
    const platform = manifest.platform;
    assert.ok(platforms[platform], '未知产物平台');
    assert.ok(!selected.has(platform), '重复的平台产物');
    selected.add(platform);
    assert.equal(manifest.schema, 1);
    for (const key of ['version', 'tag', 'identifier', 'productName'])
      assert.equal(manifest[key], meta[key], `产物 ${key} 不一致`);
    assert.equal(manifest.commit, commit, '安装包来自不同提交');
    assert.equal(String(manifest.runId), String(runId), '安装包来自不同构建');
    assert.deepEqual(manifest.validation, [
      'rustfmt',
      'cargo-test-workspace',
      'frontend-build',
      'installed-package-self-check',
    ]);
    const expected = [
      `ExplainCraft_${meta.version}_${platforms[platform].suffix}`,
      `LICENSE-${platform}.txt`,
      `SELF-CHECK-${platform}.json`,
    ];
    assert.deepEqual(
      manifest.files.map((f) => f.name),
      expected,
    );
    for (const asset of manifest.files) {
      assert.equal(path.basename(asset.name), asset.name);
      const source = path.join(path.dirname(file), asset.name);
      assert.equal(digest(source), asset.sha256, `SHA256 不匹配：${asset.name}`);
      assert.ok(!copies.has(asset.name), '重复的文件名');
      copies.set(asset.name, source);
    }
    const check = JSON.parse(fs.readFileSync(copies.get(`SELF-CHECK-${platform}.json`), 'utf8'));
    assert.equal(check.status, 'passed');
    assert.equal(check.version, meta.version);
    assert.equal(check.identifier, meta.identifier);
    assert.equal(check.productName, meta.productName);
    assert.equal(check.media, 'h264+aac');
    assert.equal(check.bundledMusic, true);
    assert.equal(check.os, platforms[platform].os);
    assert.equal(check.arch, platforms[platform].arch);
    const sums = path.join(path.dirname(file), `SHA256SUMS-${platform}.txt`);
    assert.equal(
      fs.readFileSync(sums, 'utf8'),
      manifest.files.map((f) => `${f.sha256}  ${f.name}\n`).join(''),
    );
    copies.set(`BUILD-${platform}.json`, file);
    copies.set(path.basename(sums), sums);
  }
  if (channel === 'stable')
    assert.ok(
      releasePlatforms.every((platform) => selected.has(platform)),
      '正式版必须具备 Windows、Apple Silicon Mac 和 Intel Mac 的验证产物',
    );
  assert.ok(
    !fs.existsSync(outputDir) || fs.readdirSync(outputDir).length === 0,
    '输出目录必须为空，禁止覆盖既有文件',
  );
  fs.mkdirSync(outputDir, { recursive: true });
  for (const [name, source] of copies) fs.copyFileSync(source, path.join(outputDir, name));
  if (channel === 'stable')
    fs.writeFileSync(
      path.join(outputDir, 'SHA256SUMS.txt'),
      [...copies.keys()]
        .sort()
        .map((name) => `${digest(path.join(outputDir, name))}  ${name}\n`)
        .join(''),
    );
  return [...selected].sort();
}

/** 检查 GitHub 的真实结果后才允许创建标签；已有正式版和不同内容的同名文件一律拒绝。 */
function publish(runId, channel, meta) {
  assert.match(runId, /^\d+$/);
  assert.ok(['preview', 'stable'].includes(channel));
  const repo = process.env.GITHUB_REPOSITORY;
  assert.equal(repo, 'LDFei/Bohack-ExplainCraft');
  const api = (endpoint) => JSON.parse(run('gh', ['api', `repos/${repo}/${endpoint}`]));
  const build = api(`actions/runs/${runId}`);
  const commit = run('git', ['rev-parse', 'HEAD']);
  assert.equal(build.head_sha, commit, '必须发布当前检出的已验证提交');
  const sourceBranch = process.env.GITHUB_REF_NAME ?? 'main';
  assert.ok(
    sourceBranch === 'main' ||
      (sourceBranch === 'codex/release-v0.1.0' && meta.version === '0.1.0'),
    '只允许正式分支或本次 v0.1.0 发布分支',
  );
  assert.equal(build.head_branch, sourceBranch);
  assert.equal(build.name, 'Desktop release');
  if (channel === 'stable') assert.equal(build.conclusion, 'success', '完整打包工作流尚未成功');
  const checks = api(`commits/${commit}/check-runs?per_page=100`).check_runs;
  for (const name of ['Cargo format and test', 'Frontend format, test and build']) {
    const check = checks.filter((c) => c.name === name).sort((a, b) => b.id - a.id)[0];
    assert.equal(check?.conclusion, 'success', `${name} 尚未通过，禁止发布`);
  }
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'explaincraft-release-'));
  try {
    const artifactsDir = path.join(scratch, 'artifacts');
    const outputDir = path.join(scratch, 'ready');
    run(
      'gh',
      ['run', 'download', runId, '--repo', repo, '--pattern', 'desktop-*', '--dir', artifactsDir],
      { timeout: 600_000 },
    );
    const selected = validateArtifacts({ artifactsDir, outputDir, meta, commit, runId, channel });
    const jobs = api(`actions/runs/${runId}/jobs?per_page=100`).jobs;
    for (const platform of selected)
      assert.ok(
        jobs.some((j) => j.name.endsWith(`, ${platform})`) && j.conclusion === 'success'),
        `${platform} 原生检查未通过`,
      );
    const releases = api('releases?per_page=100');
    const existing = releases.find((r) => r.tag_name === meta.tag);
    const tags = api(`git/matching-refs/tags/${meta.tag}`);
    const tag = tags.find((t) => t.ref === `refs/tags/${meta.tag}`);
    if (tag) {
      let ref = tag.object;
      if (ref.type === 'tag') ref = api(`git/tags/${ref.sha}`).object;
      assert.equal(ref.sha, commit, '已有标签指向不同提交，禁止覆盖');
    }
    if (existing) {
      assert.ok(
        existing.draft || existing.prerelease,
        '已有正式 Release，必须增加版本号，禁止覆盖',
      );
      assert.ok(tag, '已有 Release 的标签不存在');
    }
    const files = fs.readdirSync(outputDir).sort();
    const uploads = [];
    for (const name of files) {
      const old = existing?.assets.find((a) => a.name === name);
      if (old)
        assert.equal(
          old.digest,
          `sha256:${digest(path.join(outputDir, name))}`,
          `已有同名文件内容不同，禁止覆盖：${name}`,
        );
      else uploads.push(path.join(outputDir, name));
    }
    if (existing && channel === 'stable')
      assert.ok(
        existing.assets.every((a) => files.includes(a.name)),
        '现有 Release 含未验证文件',
      );
    const notes = path.join(scratch, 'notes.md');
    const details = fs.readFileSync(
      path.join(root, `front/docs/RELEASE-${meta.version}.md`),
      'utf8',
    );
    fs.writeFileSync(
      notes,
      `${details}\n\n本次已验证并提供下载：${selected.join('、')}。\n\n原生单元/集成测试、真实安装包自检和 H.264/AAC 媒体编码通过；未宣称完成所有机器上的人工 UI 与云模型测试。\n\n构建：${build.html_url}\n提交：${commit}\n`,
    );
    if (!existing) {
      // 先创建草稿，全部校验及上传成功后才公开；从不使用 --clobber。
      run('gh', [
        'release',
        'create',
        meta.tag,
        '--repo',
        repo,
        '--draft',
        '--target',
        commit,
        '--title',
        `ExplainCraft ${meta.tag}`,
        '--notes-file',
        notes,
      ]);
    }
    if (uploads.length)
      run('gh', ['release', 'upload', meta.tag, ...uploads, '--repo', repo], { timeout: 600_000 });
    run('gh', [
      'release',
      'edit',
      meta.tag,
      '--repo',
      repo,
      '--draft=false',
      `--prerelease=${channel === 'preview'}`,
      `--latest=${channel === 'stable'}`,
      '--title',
      `ExplainCraft ${meta.tag}${channel === 'preview' ? ' · 桌面预览' : ' · 跨平台桌面版'}`,
      '--notes-file',
      notes,
    ]);
    console.log(`https://github.com/${repo}/releases/tag/${meta.tag}`);
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const meta = metadata();
  const command = process.argv[2];
  const platform = process.env.RELEASE_PLATFORM;
  if (command === 'metadata') console.log(JSON.stringify(meta));
  else if (command === 'verify-package') verifyPackage(platform, meta);
  else if (command === 'collect') collect(platform, meta);
  else if (command === 'publish') publish(process.argv[3], process.argv[4], meta);
  else throw new Error('使用 metadata、verify-package、collect 或 publish 命令');
}
