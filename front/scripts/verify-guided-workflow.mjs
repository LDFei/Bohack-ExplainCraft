import assert from 'node:assert/strict';
import { chromium } from '@playwright/test';
import { createServer } from 'vite';
import { mkdir } from 'node:fs/promises';
import { createServer as createPortServer } from 'node:net';

// 使用独立临时端口，不干扰开发中的桌面应用。IPC 替身仅验证界面交互。
const probe = createPortServer();
await new Promise((resolve, reject) => {
  probe.once('error', reject);
  probe.listen(0, '127.0.0.1', resolve);
});
const testPort = probe.address().port;
await new Promise((resolve) => probe.close(resolve));
const server = await createServer({
  mode: 'studio',
  server: { host: '127.0.0.1', port: testPort, open: false },
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
  // 复制路径只验证调用与内容，不依赖浏览器剪贴板权限。
  Object.defineProperty(navigator, 'clipboard', {
    value: {
      writeText: async (text) => {
        state.copied = text;
      },
    },
  });
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
  const ok = (data, status = 200) => ({
    status,
    body: { data: structuredClone(data), request_id: 'review' },
  });
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
                asset('upload-video', 'upload', 'video', '本地视频.mp4'),
                asset('intermediate', 'generated', 'video', '隐藏中间镜头.mp4'),
              ]
            : [],
        };
      };
      state.rows.project ??= make('project', true);
      state.rows.project.draft.shots = state.rows.project.draft.shots.length
        ? state.rows.project.draft.shots
        : [
            {
              id: 'shot-1',
              duration_ms: 5000,
              narration: '概率不等于保证。',
              visual_prompt: '几何关系',
              shot_size: '中景',
              camera_motion: '固定',
              asset_ids: [],
              source_in_ms: 0,
            },
          ];
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
        case 'answer_job': {
          const old = state.job.question;
          const serial = (state.serial = (state.serial ?? 0) + 1);
          const set = (step, prompt, options, type = 'review') => {
            state.job.question = {
              id: `${step}-${serial}`,
              skill_id: step,
              step: 6,
              total_steps: 12,
              input_type: type,
              prompt,
              options: options.map(([value, label]) => ({ value, label })),
            };
          };
          const script = () =>
            set('script', '请审阅文案：概率不等于保证。', [
              ['continue', '采用文案'],
              ['edit', '按修改意见调整'],
              ['regenerate', '重新生成此步骤'],
              ['back', '返回上一项'],
            ]);
          if (old.skill_id === 'style')
            set(
              'duration',
              '选择时长',
              [
                ['30', '30 秒'],
                ['60', '60 秒'],
              ],
              'single_select',
            );
          else if (old.skill_id === 'duration')
            set(
              'music',
              '选择背景音乐来源',
              [
                ['upload', '从本地选择音乐'],
                ['ai', 'AI 生成音乐'],
                ['fixed', '原版 Vibe 示例音乐'],
              ],
              'single_select',
            );
          else if (old.skill_id === 'music' && args.value === 'upload') {
            set(
              'music',
              '选择并试听本项目已上传音频',
              [
                ['ready', '已上传，刷新音频列表'],
                ['back', '返回音乐来源'],
              ],
              'single_select',
            );
            state.job.question.id = 'skill:music_asset:1:ui-test';
          } else if (old.skill_id === 'music' && old.id.startsWith('skill:music_asset:')) {
            if (args.value === 'ready') {
              state.job.question = old;
            } else if (args.value === 'back')
              set(
                'music',
                '选择背景音乐来源',
                [
                  ['upload', '从本地选择音乐'],
                  ['fixed', '原版 Vibe 示例音乐'],
                ],
                'single_select',
              );
            else if (
              row.assets.some(
                (a) => a.id === args.value && a.source === 'upload' && a.kind === 'audio',
              )
            )
              script();
            else throw new Error('Invalid music candidate');
          } else if (old.skill_id === 'music') script();
          else if (old.skill_id === 'script' && args.value === 'continue')
            set('storyboard', '审阅分镜', [
              ['continue', '采用分镜'],
              ['save_shots', '保存逐镜修改并重新审阅'],
              ['edit', '按修改意见调整'],
              ['back', '返回上一项'],
            ]);
          else if (old.skill_id === 'script') {
            script();
            state.job.question.prompt = '已按修改意见更新文案，请重新审阅。';
          } else if (old.skill_id === 'storyboard' && args.value === 'save_shots') {
            row.draft.shots = JSON.parse(args.instruction);
            set('storyboard', '逐镜修改已保存，请重新审阅。', [
              ['continue', '采用分镜'],
              ['save_shots', '保存逐镜修改并重新审阅'],
              ['edit', '按修改意见调整'],
              ['back', '返回上一项'],
            ]);
          } else if (old.skill_id === 'storyboard' && args.value === 'back') script();
          return ok({ job_status: 'waiting_input' });
        }
        case 'upload_asset': {
          const raw = new TextDecoder().decode(args);
          const name = raw.match(/filename="([^"]+)"/)?.[1] ?? '本地音乐.wav';
          const asset = {
            id: 'new-local-music',
            project_id: 'project',
            kind: 'audio',
            source: 'upload',
            file_name: name,
            status: 'ready',
            mime_type: 'audio/wav',
            size_bytes: 2000,
            duration_ms: 5000,
            width: null,
            height: null,
            thumbnail_url: null,
            error: null,
          };
          state.rows.project.assets.push(asset);
          return ok({ asset }, 201);
        }
        case 'preview_asset':
          return ok({ playback_url: '/media/coffee-16x9.mp4', expires_at: '2027-01-01T00:00:00Z' });
        case 'runtime_status':
          return {
            items: [
              { name: 'FFmpeg', ready: true, required: true, message: '可用' },
              { name: 'ffprobe', ready: true, required: true, message: '可用' },
            ],
            music_configured: false,
            voice_configured: true,
            script_configured: true,
          };
        case 'project_events': {
          const changed = !!state.changed;
          state.changed = false;
          return { changed, cursor: changed ? '1' : '0' };
        }
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
          return ok({ download_url: '/media/ambient.wav', file_name: '上传音乐.wav' });
        // 文件位置由后端解析；替身返回固定真实路径，界面不得自行拼接。
        case 'video_location':
          if (args.projectId !== 'project' || args.versionId !== 'version-1')
            throw new Error(`unexpected location args: ${JSON.stringify(args)}`);
          return {
            path: '/Users/demo/Library/Application Support/lingjing/storage/project/rendered-video.mp4',
            directory: '/Users/demo/Library/Application Support/lingjing/storage/project',
            fileName: '最终成片.mp4',
            exists: true,
          };
        case 'reveal_video_file':
          return undefined;
        case 'get_model_settings':
          return {
            script: {
              provider: 'custom',
              model: 'test',
              baseUrl: 'http://localhost',
              hasApiKey: true,
            },
            visual: {
              provider: 'custom',
              model: 'test',
              baseUrl: 'http://localhost',
              hasApiKey: true,
              overridden: false,
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
        case 'list_models':
          return ['test-model'];
        case 'list_voices':
          return ['Cherry'];
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
  // 当前版本的真实文件位置：只调用后端查询，不在前端拼接路径。
  const previewPanel = page.getByLabel('视频作品与设置');
  await previewPanel.waitFor();
  await previewPanel.getByRole('button', { name: '文件位置', exact: true }).click();
  const locationPath =
    '/Users/demo/Library/Application Support/lingjing/storage/project/rendered-video.mp4';
  await page.getByText(locationPath, { exact: true }).waitFor();
  await page.getByRole('button', { name: '复制路径', exact: true }).click();
  assert.equal(await page.evaluate(() => window.__review.copied), locationPath);
  await page.getByRole('button', { name: '打开所在文件夹', exact: true }).click();
  assert.ok(
    await page.evaluate(() =>
      window.__review.calls.some(
        (c) => c.command === 'reveal_video_file' && c.args.versionId === 'version-1',
      ),
    ),
  );
  await page.getByRole('button', { name: '极简信息图', exact: true }).click();
  await page.getByRole('button', { name: '30 秒', exact: true }).waitFor();
  assert.equal(await page.getByRole('button', { name: '确认选择并继续' }).count(), 0);
  assert.equal(await page.getByLabel('目标视频时长').count(), 0);
  await page.getByRole('button', { name: '30 秒', exact: true }).click();
  await page.getByRole('button', { name: '从本地选择音乐', exact: true }).waitFor();
  await page.getByRole('button', { name: '从本地选择音乐', exact: true }).click();
  await page.getByRole('button', { name: '上传新音频', exact: true }).waitFor();
  await page.locator('.question-card input[type=file]').setInputFiles({
    name: '我的节奏.wav',
    mimeType: 'audio/wav',
    buffer: Buffer.from('RIFF-local-browser-test'),
  });
  await page
    .locator('.local-music-list')
    .getByRole('button', { name: /我的节奏.wav/ })
    .click();
  await page.getByLabel('所选背景音乐试听').waitFor();
  await page.getByRole('button', { name: '使用这首音乐', exact: true }).click();
  await page.getByLabel('当前步骤修改意见').fill('保留结论，换一个抛硬币的例子');
  const music = await page.evaluate(() =>
    window.__review.calls
      .filter((c) => c.command === 'answer_job' && c.args.value === 'new-local-music')
      .at(-1),
  );
  assert.equal(music.args.questionId, 'skill:music_asset:1:ui-test');
  await page.getByRole('button', { name: '按修改意见调整', exact: true }).click();
  await page.getByText('已按修改意见更新文案，请重新审阅。', { exact: true }).waitFor();
  assert.equal(await page.getByLabel('当前步骤修改意见').inputValue(), '');
  const edit = await page.evaluate(() =>
    window.__review.calls
      .filter((c) => c.command === 'answer_job' && c.args.value === 'edit')
      .at(-1),
  );
  assert.equal(edit.args.instruction, '保留结论，换一个抛硬币的例子');
  await page.getByRole('button', { name: '采用文案', exact: true }).click();
  await page.getByText('逐镜头编辑文字、顺序、素材和视频入点', { exact: true }).click();
  await page.getByLabel('镜头1解说').fill('独立事件不会记住过去的结果。');
  await page.getByRole('button', { name: '保存逐镜修改并重新审阅', exact: true }).click();
  await page.getByText('逐镜修改已保存，请重新审阅。', { exact: true }).waitFor();
  const shotSave = await page.evaluate(() =>
    window.__review.calls
      .filter((c) => c.command === 'answer_job' && c.args.value === 'save_shots')
      .at(-1),
  );
  assert.equal(JSON.parse(shotSave.args.instruction)[0].narration, '独立事件不会记住过去的结果。');
  await page.getByRole('button', { name: '返回上一项', exact: true }).click();
  await page.getByRole('button', { name: '采用文案', exact: true }).waitFor();
  await page
    .locator('details.progress-card')
    .evaluateAll((nodes) => nodes.forEach((n) => (n.open = true)));
  await page.getByText('后端已保存创作方向').waitFor();
  // 上传视频通过独立播放器打开，完全不要求关联生成版本。
  await page.evaluate(() => {
    location.hash = '#/projects/project/library';
  });
  await page.getByRole('button', { name: '预览 本地视频.mp4', exact: true }).click();
  const player = page.getByLabel('上传视频播放器');
  await player.waitFor();
  await page.waitForFunction(
    () => document.querySelector('video[aria-label="上传视频播放器"]')?.readyState >= 2,
  );
  await player.evaluate((video) => video.play());
  assert.equal(await player.evaluate((video) => video.paused), false);
  await page.keyboard.press('Escape');
  await player.waitFor({ state: 'detached' });
  assert.equal(await player.count(), 0);
  assert.ok(
    await page.evaluate(() =>
      window.__review.calls.some(
        (c) => c.command === 'preview_asset' && c.args.assetId === 'upload-video',
      ),
    ),
  );
  // 音乐配置入口应定位音乐模块，不能跳到 Breeze 下载。
  await page.evaluate(() => {
    window.__review.job.question = {
      id: 'music-settings',
      skill_id: 'music',
      step: 5,
      total_steps: 12,
      input_type: 'single_select',
      prompt: 'AI音乐需要配置',
      options: [
        { value: 'open_settings', label: '打开音乐 API 设置' },
        { value: 'ready', label: '已配置，重新检查' },
      ],
    };
    window.__review.changed = true;
    location.hash = '#/projects/project';
  });
  await page.getByRole('button', { name: '打开音乐 API 设置', exact: true }).click();
  await page.getByLabel('音乐供应商').waitFor({ state: 'visible' });
  await page.getByLabel('音乐供应商').selectOption('minimax');
  assert.equal(await page.getByLabel('音乐模型名称').inputValue(), 'music-3.0');
  await page.getByRole('button', { name: '关闭对话框', exact: true }).click();
  const probes = await page.evaluate(
    () => window.__review.calls.filter((c) => c.command === 'breeze_model_status').length,
  );
  await page.getByRole('button', { name: '已配置，重新检查', exact: true }).click();
  await page.getByRole('button', { name: '采用文案', exact: true }).waitFor();
  assert.equal(
    await page.evaluate(
      () => window.__review.calls.filter((c) => c.command === 'breeze_model_status').length,
    ),
    probes,
  );
  await mkdir('test-results/vibe-v2', { recursive: true });
  await page.screenshot({ path: 'test-results/vibe-v2/workflow-desktop.png' });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: 'test-results/vibe-v2/workflow-mobile.png' });
  assert.ok(
    await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 2),
    '移动页面不能横向溢出',
  );
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.getByRole('button', { name: '结束此次话题', exact: true }).click();
  await page.waitForFunction(() => location.hash === '#/create');
  assert.ok(
    await page.evaluate(() => window.__review.calls.some((c) => c.command === 'cancel_job')),
  );
  const input = page.getByRole('textbox', { name: '视频创作需求' });
  await input.fill('未发送内容');
  await page.getByRole('button', { name: '新建话题', exact: true }).click();
  assert.equal(await input.inputValue(), '');
  await page.getByRole('button', { name: '灵感模板', exact: true }).click();
  await page.waitForFunction(() => {
    const images = [...document.querySelectorAll('.template-card img')];
    return images.length === 3 && images.every((i) => i.complete && i.naturalWidth > 0);
  });
  assert.deepEqual(errors, []);
  console.log(
    'PASS: direct choices, local music upload/audition/select, review edits, storyboard save/back, uploaded video playback, music settings focus, voice isolation, logs, mobile layout, topic lifecycle, template assets',
  );
} catch (error) {
  await mkdir('test-results/vibe-v2', { recursive: true });
  await page.screenshot({ path: 'test-results/vibe-v2/failure.png' });
  console.log(
    await page.evaluate(() => ({
      question: window.__review.job.question,
      commands: window.__review.calls.slice(-12).map((c) => c.command),
      errorText: document.querySelector('.question-card')?.innerText,
      pageErrors: [],
    })),
  );
  throw error;
} finally {
  await browser.close();
  await server.close();
}
