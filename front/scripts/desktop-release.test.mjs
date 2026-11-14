// 验证发布保护的失败路径：缺平台、篡改、错误来源和已有文件不能混入正式版。
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  digest,
  metadata,
  packageEnvironment,
  platforms,
  releasePlatforms,
  validateArtifacts,
} from './desktop-release.mjs';

/** 构造隔离产物，测试只用字节替身，不上传、不连接 GitHub。 */
function fixture(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'explaincraft-release-test-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  const artifactsDir = path.join(directory, 'artifacts');
  const outputDir = path.join(directory, 'ready');
  fs.mkdirSync(artifactsDir);
  const meta = metadata();
  const commit = 'a'.repeat(40);
  const runId = '123';
  /** 为一个平台写入包含来源与原生自检证据的完整清单。 */
  function add(platform) {
    const target = path.join(artifactsDir, platform);
    fs.mkdirSync(target);
    const names = [
      `ExplainCraft_${meta.version}_${platforms[platform].suffix}`,
      `LICENSE-${platform}.txt`,
      `SELF-CHECK-${platform}.json`,
    ];
    fs.writeFileSync(path.join(target, names[0]), 'test installer');
    fs.writeFileSync(path.join(target, names[1]), 'test license');
    fs.writeFileSync(
      path.join(target, names[2]),
      JSON.stringify({
        ...meta,
        ...platforms[platform],
        status: 'passed',
        media: 'h264+aac',
        bundledMusic: true,
      }),
    );
    const files = names.map((name) => ({ name, sha256: digest(path.join(target, name)) }));
    const manifest = {
      schema: 1,
      ...meta,
      platform,
      commit,
      runId,
      validation: [
        'rustfmt',
        'cargo-test-workspace',
        'frontend-build',
        'installed-package-self-check',
      ],
      files,
    };
    fs.writeFileSync(path.join(target, `BUILD-${platform}.json`), JSON.stringify(manifest));
    fs.writeFileSync(
      path.join(target, `SHA256SUMS-${platform}.txt`),
      files.map((f) => `${f.sha256}  ${f.name}\n`).join(''),
    );
    return path.join(target, names[0]);
  }
  return { artifactsDir, outputDir, meta, commit, runId, add };
}

test('partial verified artifacts can be previewed but cannot become stable', (t) => {
  const f = fixture(t);
  f.add('windows-x64');
  assert.throws(
    () => validateArtifacts({ ...f, channel: 'stable' }),
    /Windows、Apple Silicon Mac 和 Intel Mac/,
  );
  assert.ok(!fs.existsSync(f.outputDir));
  assert.deepEqual(validateArtifacts({ ...f, channel: 'preview' }), ['windows-x64']);
});

test('stable release requires Windows and both Mac packages and writes complete checksums', (t) => {
  const f = fixture(t);
  for (const p of releasePlatforms) f.add(p);
  assert.equal(validateArtifacts({ ...f, channel: 'stable' }).length, 3);
  const sums = fs.readFileSync(path.join(f.outputDir, 'SHA256SUMS.txt'), 'utf8');
  for (const p of releasePlatforms)
    assert.ok(sums.includes(`ExplainCraft_${f.meta.version}_${platforms[p].suffix}`));
});

test('stable release rejects every missing required native platform', (t) => {
  for (const missing of releasePlatforms) {
    const f = fixture(t);
    for (const platform of releasePlatforms) if (platform !== missing) f.add(platform);
    assert.throws(() => validateArtifacts({ ...f, channel: 'stable' }), /正式版必须具备/);
    assert.ok(!fs.existsSync(f.outputDir));
  }
});

test('metadata rejects a Rust lockfile with a different application version', (t) => {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'explaincraft-metadata-test-'));
  t.after(() => fs.rmSync(base, { recursive: true, force: true }));
  const source = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
  for (const name of [
    'front/package.json',
    'front/package-lock.json',
    'src-tauri/tauri.conf.json',
    'src-tauri/Cargo.toml',
    'Cargo.lock',
  ]) {
    const destination = path.join(base, name);
    fs.mkdirSync(path.dirname(destination), { recursive: true });
    fs.copyFileSync(path.join(source, name), destination);
  }
  const lockfile = path.join(base, 'Cargo.lock');
  fs.writeFileSync(
    lockfile,
    fs
      .readFileSync(lockfile, 'utf8')
      .replace(
        /name = "lingjing-desktop"\nversion = "[^"]+"/,
        'name = "lingjing-desktop"\nversion = "9.9.9"',
      ),
  );
  assert.throws(() => metadata(base), /前后端版本不一致/);
});

test('tampered installer is rejected before copying any files', (t) => {
  const f = fixture(t);
  fs.appendFileSync(f.add('macos-arm64'), 'tampered');
  assert.throws(() => validateArtifacts({ ...f, channel: 'preview' }), /SHA256/);
  assert.ok(!fs.existsSync(f.outputDir));
});

test('a package from another commit or run cannot be published', (t) => {
  const f = fixture(t);
  f.add('linux-x64');
  assert.throws(
    () => validateArtifacts({ ...f, commit: 'b'.repeat(40), channel: 'preview' }),
    /不同提交/,
  );
  assert.throws(() => validateArtifacts({ ...f, runId: '124', channel: 'preview' }), /不同构建/);
});

test('existing output is preserved instead of overwritten', (t) => {
  const f = fixture(t);
  f.add('macos-x64');
  fs.mkdirSync(f.outputDir);
  fs.writeFileSync(path.join(f.outputDir, 'keep.txt'), 'preserve');
  assert.throws(() => validateArtifacts({ ...f, channel: 'preview' }), /禁止覆盖/);
  assert.equal(fs.readFileSync(path.join(f.outputDir, 'keep.txt'), 'utf8'), 'preserve');
});

test('installed package cannot borrow FFmpeg DLLs or executables from the build SDK', () => {
  const base = {
    Path: 'C:\\sdk\\bin',
    PATH: 'C:\\another-sdk',
    SystemRoot: 'C:\\Windows',
    FFMPEG_PATH: 'C:\\sdk\\ffmpeg.exe',
    DYLD_LIBRARY_PATH: '/opt/homebrew/lib',
  };
  const windows = packageEnvironment('windows-x64', 'C:\\installed\\lingjing.exe', base);
  assert.equal(windows.PATH, 'C:\\installed;C:\\Windows\\System32;C:\\Windows');
  assert.equal(windows.Path, undefined);
  assert.equal(windows.FFMPEG_PATH, undefined);
  assert.equal(windows.DYLD_LIBRARY_PATH, undefined);
  assert.equal(packageEnvironment('macos-arm64', '/app/lingjing', base).PATH, '/usr/bin:/bin');
  assert.equal(base.Path, 'C:\\sdk\\bin');
});
