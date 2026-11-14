/** 真实 Vue → 请求 → Rust 路由 → SQLite → worker → 编码器契约验收；仅模型使用本地替身。 */
import assert from 'node:assert/strict';
import { chromium } from '@playwright/test';
import { createServer } from 'vite';
import { spawn } from 'node:child_process';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const evidence = 'test-results/workflow-contract';
await mkdir(evidence, { recursive: true });
const ready = join(tmpdir(), `explaincraft-browser-${process.pid}.json`);
const backend = spawn(
  'cargo',
  [
    'test',
    '--manifest-path',
    '../src-tauri/Cargo.toml',
    '--lib',
    'browser_contract_server',
    '--',
    '--ignored',
    '--nocapture',
  ],
  { env: { ...process.env, EXPLAINCRAFT_BROWSER_READY: ready }, stdio: ['ignore', 'pipe', 'pipe'] },
);
let log = '';
backend.stdout.on('data', (b) => {
  log += b;
});
backend.stderr.on('data', (b) => {
  log += b;
});
let browser, server, page;
const results = [];
try {
  let info;
  for (let i = 0; i < 240; i++) {
    try {
      info = JSON.parse(await readFile(ready, 'utf8'));
      break;
    } catch {}
    if (backend.exitCode !== null) throw new Error(log);
    await new Promise((r) => setTimeout(r, 500));
  }
  if (!info) throw new Error('Backend did not start');
  const backendUrl = `http://127.0.0.1:${info.port}`;
  server = await createServer({
    mode: 'studio',
    server: {
      host: '127.0.0.1',
      port: 0,
      proxy: { '/backend': { target: backendUrl, rewrite: (p) => p.replace(/^\/backend/, '') } },
    },
  });
  await server.listen();
  browser = await chromium.launch({ channel: 'chrome', headless: true });
  page = await browser.newPage({ viewport: { width: 1480, height: 1000 } });
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.addInitScript(
    ({ backendUrl }) => {
      window.__contractCalls = [];
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      window.__TAURI_INTERNALS__ = {
        convertFileSrc: (u) => u,
        transformCallback: () => 1,
        invoke: async (command, args = {}, options = {}) => {
          window.__contractCalls.push({
            command,
            args: args instanceof Uint8Array ? 'binary' : structuredClone(args),
          });
          const get = async (path, method = 'GET', body) => {
            const headers = { ...options.headers };
            let payload;
            if (body !== undefined) {
              headers['Content-Type'] = 'application/json';
              payload = JSON.stringify(body);
            }
            const r = await fetch('/backend' + path, { method, headers, body: payload });
            const text = await r.text();
            return { status: r.status, body: text ? JSON.parse(text) : null };
          };
          const p = `/api/v1/projects/${args.projectId}`;
          const j = `/api/v1/jobs/${args.jobId}`;
          switch (command) {
            case 'capabilities':
              return get('/api/v1/capabilities');
            case 'list_projects':
              return get('/api/v1/projects');
            case 'create_project':
              return get('/api/v1/projects', 'POST', {});
            case 'get_project':
              return get(p);
            case 'patch_draft':
              return get(p + '/draft', 'PATCH', args.input);
            case 'send_message':
              return get(p + '/messages', 'POST', args.input);
            case 'list_assets':
              return get(p + '/assets');
            case 'get_asset':
              return get(`/api/v1/assets/${args.assetId}`);
            case 'list_versions':
              return get(p + '/versions');
            case 'get_version':
              return get(p + '/versions/' + args.versionId);
            case 'get_job':
              return get(j);
            case 'cancel_job':
              return get(j + '/cancel', 'POST', {});
            case 'retry_job':
              return get(j + '/retry', 'POST', {
                expected_draft_revision: args.expectedDraftRevision,
              });
            case 'answer_job':
              return get(j + '/answers', 'POST', {
                question_id: args.questionId,
                value: args.value,
                instruction: args.instruction ?? '',
                asset_id: args.assetId ?? null,
              });
            case 'project_events': {
              const r = await get(p);
              const cursor = r.body.data.event_cursor;
              return { changed: cursor !== args.after, cursor };
            }
            case 'runtime_status':
              return (await fetch('/backend/__test/runtime')).json();
            case 'get_model_settings':
              throw new Error('No cloud settings in isolated contract test');
            case 'list_voices':
              return [];
            case 'download_asset':
              return get(`/api/v1/assets/${args.assetId}/download`);
            case 'preview_asset':
              return get(`/api/v1/assets/${args.assetId}/preview`);
            case 'export_video':
              return get(p + '/versions/' + args.versionId + '/exports', 'POST', {
                resolution: args.resolution,
                container: 'mp4',
                fps: 30,
                include_project: args.includeProject,
              });
            case 'upload_asset': {
              const headers = {
                ...options.headers,
                'Content-Type': options.headers['X-Upload-Content-Type'],
              };
              const r = await fetch(
                '/backend/api/v1/projects/' + options.headers['X-Project-Id'] + '/assets',
                { method: 'POST', headers, body: new Uint8Array(args) },
              );
              return { status: r.status, body: await r.json() };
            }
            case 'plugin:event|listen':
              return 1;
            case 'plugin:event|unlisten':
              return;
            default:
              throw new Error('Unmapped command ' + command);
          }
        },
      };
    },
    { backendUrl },
  );
  const base = `http://127.0.0.1:${server.httpServer.address().port}/studio.html`;
  const card = () => page.locator('.question-card').filter({ has: page.locator('.question-step') });
  const click = async (name) => {
    const b = card().getByRole('button', { name, exact: true });
    await b.waitFor({ timeout: 60000 });
    await b.click();
  };
  const wav = Buffer.alloc(44 + 48000 * 2);
  wav.write('RIFF', 0);
  wav.writeUInt32LE(wav.length - 8, 4);
  wav.write('WAVEfmt ', 8);
  wav.writeUInt32LE(16, 16);
  wav.writeUInt16LE(1, 20);
  wav.writeUInt16LE(1, 22);
  wav.writeUInt32LE(48000, 24);
  wav.writeUInt32LE(96000, 28);
  wav.writeUInt16LE(2, 32);
  wav.writeUInt16LE(16, 34);
  wav.write('data', 36);
  wav.writeUInt32LE(wav.length - 44, 40);
  for (let i = 0; i < 48000; i++)
    wav.writeInt16LE(Math.sin((i * 2 * Math.PI * 440) / 48000) * 7000, 44 + i * 2);
  async function start() {
    await page.goto(base + '#/create');
    await page
      .getByRole('textbox', { name: '视频创作需求' })
      .fill('用具体对象讲清知识，保留自定义要求。');
    await page.getByRole('button', { name: '生成视频', exact: true }).click();
    for (const name of [
      '极简信息图',
      '15 秒 · 一个结论',
      'SVG 动画 · 矢量图解',
      '不用配音，只保留 BGM',
    ])
      await click(name);
    await card().getByText('选择背景音乐来源', { exact: false }).waitFor();
  }
  async function finish() {
    for (const name of [
      '采用方案，开始文案',
      '采用文案，安排分镜',
      '采用分镜',
      '开始制作与合成',
      '保留初版，不使用剪辑库',
      '完成并保存作品',
    ])
      await click(name);
    await page.waitForFunction(() => !document.querySelector('.question-step'), { timeout: 60000 });
    await page.locator('video').waitFor({ timeout: 30000 });
    await page.waitForFunction(() => document.querySelector('video')?.readyState >= 2);
    assert.ok(await page.locator('video').count());
  }
  for (const mode of ['default', 'late-upload', 'existing', 'upload-failure', 'missing-api']) {
    await start();
    if (mode === 'existing') {
      await page
        .getByLabel('选择创作素材文件')
        .setInputFiles({ name: '已有音乐.wav', mimeType: 'audio/wav', buffer: wav });
      await page.waitForFunction(
        () =>
          document.querySelector('.toast')?.textContent?.includes('失败') ||
          !document.querySelector('[role=status]')?.textContent?.includes('上传'),
      );
    }
    if (['late-upload', 'existing', 'upload-failure'].includes(mode)) {
      await click('从本地选择音乐 / 使用素材库音频');
      await card().getByRole('button', { name: '返回音乐来源', exact: true }).waitFor();
      if (mode === 'late-upload') {
        // 取消文件选择器不回答问题，仍留在同一可恢复的音乐卡。
        const chooser = page.waitForEvent('filechooser');
        await click('上传新音频');
        await (await chooser).setFiles([]);
        await card().getByRole('button', { name: '返回音乐来源', exact: true }).waitFor();
        await card()
          .locator('input[type=file]')
          .setInputFiles({ name: '等待时上传.wav', mimeType: 'audio/wav', buffer: wav });
        await card()
          .getByRole('button', { name: /等待时上传.wav/ })
          .waitFor();
        await page.reload();
        await card()
          .getByRole('button', { name: /等待时上传.wav/ })
          .waitFor();
      }
      if (mode === 'upload-failure') {
        await card()
          .locator('input[type=file]')
          .setInputFiles({
            name: '损坏.wav',
            mimeType: 'audio/wav',
            buffer: Buffer.from('invalid'),
          });
        await page.getByRole('button', { name: '返回音乐来源', exact: true }).waitFor();
        await click('返回音乐来源');
        await click('本地程序化配乐（无需 API）');
      } else {
        await card()
          .getByRole('button', { name: mode === 'existing' ? /已有音乐.wav/ : /等待时上传.wav/ })
          .click();
        // 播放真实后端签名音频；进入下一问题时旧节点必须停止。
        await page.getByLabel('所选背景音乐试听').evaluate(async (audio) => {
          window.__lastAudition = audio;
          await audio.play();
        });
        await page.waitForFunction(() => window.__lastAudition?.currentTime > 0.1);
        await click('使用这首音乐');
        await page.waitForFunction(() => window.__lastAudition?.paused === true);
      }
    } else if (mode === 'missing-api') {
      await click('AI 生成音乐（使用已配置的音乐 API）');
      await card()
        .getByText(/尚未配置音乐 API/)
        .waitFor();
      await click('返回音乐来源');
      await click('本地程序化配乐（无需 API）');
    } else {
      const choice = card().getByRole('button', {
        name: '本地程序化配乐（无需 API）',
        exact: true,
      });
      await choice.evaluate((button) => {
        button.click();
        button.click();
      });
    }
    await finish();
    const duplicateQuestions = await page.evaluate(() => {
      const counts = new Map();
      for (const c of window.__contractCalls.filter((c) => c.command === 'answer_job'))
        counts.set(c.args.questionId, (counts.get(c.args.questionId) ?? 0) + 1);
      return [...counts].filter(([, count]) => count > 1);
    });
    assert.deepEqual(duplicateQuestions, [], '快速重复点击只能提交一次当前问题');
    await page.screenshot({ path: `${evidence}/${mode}.png` });
    results.push({ mode, passed: true });
  }
  assert.deepEqual(errors, []);
  await writeFile(
    `${evidence}/results.json`,
    JSON.stringify({ results, errors, backendRoot: info.root }, null, 2),
  );
  console.log(
    'PASS: five real UI/Rust workflow paths, music subquestion, valid/invalid upload, cancel picker, reload, missing API recovery and actual rendering',
  );
} catch (error) {
  if (page) await page.screenshot({ path: `${evidence}/failure.png` });
  throw error;
} finally {
  backend.kill('SIGTERM');
  if (browser) await browser.close();
  if (server) await server.close();
  await writeFile(`${evidence}/backend.log`, log);
}
