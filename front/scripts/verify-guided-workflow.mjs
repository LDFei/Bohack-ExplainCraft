import assert from 'node:assert/strict';
import { chromium } from '@playwright/test';
import { createServer } from 'vite';

// 使用独立临时端口，不干扰开发中的桌面应用。IPC 替身仅验证界面交互。
const server = await createServer({
  mode: 'studio',
  server: { host: '127.0.0.1', port: 0, open: false },
});
await server.listen();
const address = server.httpServer.address();
const base = `http://127.0.0.1:${address.port}`;

let browser;
try {
  browser = await chromium.launch({ channel: 'chrome', headless: true });
} catch (error) {
  await server.close();
  throw error;
}
const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
const errors = [];
page.on('pageerror', (error) => errors.push(error.message));
await page.addInitScript(() => {
  const state = { calls: [], rows: {}, played: [], paused: 0, created: 0 };
  window.__review = state;
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
  // 媒体行为只记录调用；不让浏览器测试访问示例音轨或真实供应商。
  window.Audio = class {
    constructor(url) {
      this.src = url;
    }
    async play() {
      state.played.push(this.src);
    }
    pause() {
      state.paused += 1;
    }
  };
  const ok = (data, status = 200) => ({ status, body: { data, request_id: 'review' } });
  window.__TAURI_INTERNALS__ = {
    convertFileSrc: (url) => url,
    transformCallback: () => 1,
    invoke: async (command, args = {}) => {
      state.calls.push({ command, args: structuredClone(args) });
      const { capabilities, emptyDraft } = await import('/src/domain/content.ts');
      const make = (id, generated) => {
        const draft = emptyDraft();
        const created_at = '2026-11-14T10:00:00Z';
        const project = {
          id,
          title: '合并验证',
          status: generated ? 'ready' : 'draft',
          draft_revision: 1,
          active_job_id: null,
          latest_video_version_id: generated ? 'version-1' : null,
          draft_dirty: false,
          created_at,
          updated_at: created_at,
        };
        const version = {
          id: 'version-1',
          project_id: id,
          version_number: 1,
          source_draft_revision: 1,
          source_job_id: 'finished-job',
          created_at,
          video_asset_id: 'rendered-video',
          thumbnail_url: '',
          duration_ms: 15000,
          width: 1280,
          height: 720,
          playback_url: '/media/coffee-16x9.mp4',
          url_expires_at: '2027-01-01T00:00:00Z',
          snapshot: structuredClone(draft),
          exports: [],
        };
        const asset = (id, source, kind, name) => ({
          id,
          project_id: project.id,
          source,
          kind,
          file_name: name,
          status: 'ready',
          mime_type: `${kind}/${kind === 'audio' ? 'wav' : 'mp4'}`,
          size_bytes: 1024,
          duration_ms: 15000,
          width: null,
          height: null,
          thumbnail_url: null,
          error: null,
        });
        return {
          project,
          draft,
          version: generated ? version : null,
          assets: generated
            ? [
                asset('rendered-video', 'rendered', 'video', '最终成片.mp4'),
                asset('upload-audio', 'upload', 'audio', '上传音乐.wav'),
                asset('intermediate', 'generated', 'video', '隐藏中间镜头.mp4'),
              ]
            : [],
        };
      };
      state.rows.project ??= make('project', true);
      state.job ??= {
        id: 'guided-job',
        project_id: 'project',
        kind: 'generate',
        status: 'waiting_input',
        current_stage: 'planning',
        progress_percent: 0,
        stages: [
          { key: 'planning', status: 'running', message: '后端正在准备方案', progress_percent: 0 },
        ],
        logs: [
          {
            id: 'log-1',
            stage: 'planning',
            message: '后端已保存创作方向',
            created_at: '2026-11-14T10:00:00Z',
            level: 'info',
          },
        ],
        question: {
          id: 'style-question',
          skill_id: 'style',
          step: 1,
          total_steps: 12,
          input_type: 'single_select',
          prompt: '选择视频风格',
          expires_at: '2027-01-01T00:00:00Z',
          options: [
            { value: 'minimal', label: '极简信息图' },
            { value: 'warm', label: '温暖手绘' },
          ],
        },
        created_at: '2026-11-14T10:00:00Z',
      };
      const row = state.rows[args.projectId ?? 'project'];
      switch (command) {
        case 'capabilities':
          return ok(capabilities);
        case 'list_projects':
          return ok({ items: Object.values(state.rows).map((row) => row.project) });
        case 'get_project':
          return ok({
            project: row.project,
            draft: row.draft,
            active_job:
              row.project.id === 'project' && state.job.status === 'waiting_input'
                ? state.job
                : null,
            latest_video: row.version,
            event_cursor: '0',
            messages_page: {
              next_cursor: null,
              items: row.version
                ? [
                    {
                      id: 'message',
                      project_id: row.project.id,
                      role: 'assistant',
                      text: '完成',
                      status: 'completed',
                      asset_ids: [],
                      job_id: 'guided-job',
                      last_delta_seq: 0,
                      created_at: '2026-11-14T10:00:00Z',
                      blocks: [
                        { type: 'progress', job_id: 'guided-job' },
                        {
                          type: 'question',
                          job_id: 'guided-job',
                          question_id: state.job.question.id,
                        },
                        {
                          type: 'video',
                          video_version_id: 'version-1',
                          elapsed_ms: 1200,
                          total_tokens: 1234,
                          token_complete: true,
                        },
                      ],
                    },
                  ]
                : [],
            },
          });
        case 'list_assets':
          return ok({ items: row.assets });
        case 'list_versions':
          return ok({ items: row.version ? [row.version] : [] });
        case 'get_version':
          return ok({ version: row.version });
        case 'get_job':
          return ok({ job: state.job });
        case 'cancel_job':
          state.job.status = 'cancelled';
          return ok({ job_status: 'cancelled' });
        case 'answer_job':
          state.job.question = {
            ...state.job.question,
            id: 'duration-question',
            skill_id: 'duration',
            step: 2,
            prompt: '选择时长',
            options: [{ value: '30', label: '30 秒' }],
          };
          return ok({ job_status: 'waiting_input' });
        case 'project_events':
          return { changed: false, cursor: '0' };
        case 'create_project': {
          const id = `new-${++state.created}`;
          state.rows[id] = make(id, false);
          return ok({ project: state.rows[id].project }, 201);
        }
        case 'patch_draft':
          Object.assign(row.draft, args.input.changes);
          row.project.draft_revision += 1;
          return ok({ draft: row.draft, draft_revision: row.project.draft_revision });
        case 'download_asset':
          return ok({ download_url: 'data:audio/wav;base64,UklGRg==', file_name: '上传音乐.wav' });
        case 'get_model_settings':
          return {
            script: {
              provider: 'custom',
              model: 'test',
              baseUrl: 'http://localhost',
              hasApiKey: true,
            },
            music: { provider: 'procedural', model: 'test', baseUrl: '', hasApiKey: false },
            voice: {
              provider: 'qwen',
              model: 'test',
              baseUrl: '',
              hasApiKey: true,
              voiceId: 'Cherry',
            },
          };
        case 'plugin:event|listen':
          return 1;
        case 'plugin:event|unlisten':
          return;
        case 'breeze_model_status':
          return {
            installed: false,
            dir: '',
            comfyuiDir: '',
            source: 'unset',
            totalBytes: 5580631573,
            downloading: false,
          };
        default:
          throw new Error(`Unexpected command: ${command}`);
      }
    },
  };
});

try {
  await page.goto(`${base}/studio.html#/projects/project`);
  await page.getByRole('radio', { name: '极简信息图' }).waitFor();
  assert.equal(await page.getByLabel('目标视频时长').count(), 0);
  await page.getByRole('radio', { name: '极简信息图' }).check();
  await page.getByRole('button', { name: '确认选择并继续' }).click();
  await page.getByRole('radio', { name: '30 秒' }).waitFor();
  assert.ok(await page.getByRole('button', { name: '确认选择并继续' }).isDisabled());
  const saved = await page.evaluate(() =>
    window.__review.calls.find((c) => c.command === 'answer_job'),
  );
  assert.equal(saved.args.questionId, 'style-question');
  assert.equal(saved.args.value, 'minimal');
  // 同名 ready 也用于上传音频，不能误打开 Breeze 设置。
  await page.evaluate(() => {
    window.__review.job.question = {
      ...window.__review.job.question,
      id: 'music-upload-question',
      skill_id: 'music',
      prompt: '上传音乐后继续',
      options: [{ value: 'ready', label: '已上传，刷新音频列表' }],
    };
  });
  await page.evaluate(() => {
    location.hash = '#/create';
  });
  await page.waitForFunction(() => location.hash === '#/create');
  await page.evaluate(() => {
    location.hash = '#/projects/project';
  });
  await page.getByRole('radio', { name: '已上传，刷新音频列表' }).check();
  const probes = await page.evaluate(
    () => window.__review.calls.filter((c) => c.command === 'breeze_model_status').length,
  );
  await page.getByRole('button', { name: '确认选择并继续' }).click();
  await page.getByRole('radio', { name: '30 秒' }).waitFor();
  assert.equal(
    await page.evaluate(
      () => window.__review.calls.filter((c) => c.command === 'breeze_model_status').length,
    ),
    probes,
  );
  const ready = await page.evaluate(() =>
    window.__review.calls.filter((c) => c.command === 'answer_job').at(-1),
  );
  assert.equal(ready.args.value, 'ready');
  await page
    .locator('details.progress-card')
    .evaluateAll((nodes) => nodes.forEach((n) => (n.open = true)));
  await page.getByText('后端已保存创作方向').waitFor();
  await page.getByRole('button', { name: '结束此次话题', exact: true }).click();
  await page.waitForFunction(() => location.hash === '#/create');
  assert.ok(
    await page.evaluate(() => window.__review.calls.some((c) => c.command === 'cancel_job')),
  );
  const input = page.getByRole('textbox', { name: '视频创作需求' });
  await input.fill('应清空的未发送主题');
  await page.getByRole('button', { name: '新建话题', exact: true }).click();
  assert.equal(await input.inputValue(), '');
  await page.getByRole('button', { name: '灵感模板', exact: true }).click();
  await page.getByText('Ink Press · 纸墨叙事', { exact: false }).first().waitFor();
  // 预览资源必须实际加载，不能只检查文字而遗漏打包路径错误。
  await page.waitForFunction(() => {
    const images = [...document.querySelectorAll('.template-card img')];
    return images.length === 3 && images.every((image) => image.complete && image.naturalWidth > 0);
  });
  assert.deepEqual(errors, []);
  console.log(
    'PASS: choice UI, stale-answer isolation, duration removal, backend logs, end-topic cancel, new-topic reset, Shotcraft template, uploaded-music choice isolation',
  );
} finally {
  await browser.close();
  await server.close();
}
