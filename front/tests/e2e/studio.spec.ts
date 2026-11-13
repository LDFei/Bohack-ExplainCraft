/**
 * 浏览器端到端测试：从用户可见的控件操作页面，验证播放、上传、状态恢复和移动端交互。
 */
import { test, expect } from '@playwright/test';
import type { Page } from '@playwright/test';
import { resolve } from 'node:path';
import { readFile } from 'node:fs/promises';

/**
 * 从空白页输入一句话并点击生成，等待项目路由创建，供多个浏览器场景复用。
 */
async function startFresh(page: Page, text = '做一个温暖的咖啡店宣传视频') {
  await page.goto('/studio.html#/create');
  await page.getByRole('textbox', { name: '视频创作需求' }).fill(text);
  await page.getByRole('button', { name: '生成视频', exact: true }).click();
  await expect(page).toHaveURL(/\/projects\/prj_/);
}

// 通过真实播放器和浏览器下载验证成片不是空链接；下载结果必须含 MP4 文件头。
test('initial workspace plays a real local video and downloads a valid MP4', async ({ page }) => {
  // 收集未捕获的页面异常，不能只看控件存在就判断页面没有运行错误。
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/studio.html#/workspace');
  await expect(page.getByRole('textbox', { name: '视频创作需求' })).toBeVisible();
  const video = page.locator('video');
  await expect
    .poll(() =>
      video.evaluate((element) => ({
        duration: element.duration,
        width: element.videoWidth,
        height: element.videoHeight,
      })),
    )
    .toEqual({ duration: 30, width: 720, height: 1280 });
  await page.getByRole('button', { name: '播放视频', exact: true }).click();
  await expect.poll(() => video.evaluate((element) => element.currentTime)).toBeGreaterThan(0.1);
  await page.getByRole('button', { name: '导出视频', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByRole('button', { name: '导出视频', exact: true }).click();
  await expect(dialog.getByText('视频文件已准备就绪')).toBeVisible();
  // 先订阅下载事件再点击按钮，避免下载开始太快导致测试错过事件。
  const downloaded = page.waitForEvent('download');
  await dialog.getByRole('button', { name: '下载视频', exact: true }).click();
  const download = await downloaded;
  expect(download.suggestedFilename()).toMatch(/v1\.mp4$/);
  const path = await download.path();
  const bytes = await readFile(path!);
  expect(bytes.subarray(4, 8).toString()).toBe('ftyp');
  expect(errors).toEqual([]);
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
});

// 制作中整页刷新，再等任务完成，检查消息与版本都没有重复创建。
test('one-sentence generation survives refresh and produces one version', async ({ page }) => {
  await startFresh(page);
  await expect(page.getByRole('button', { name: '停止生成', exact: true })).toBeVisible();
  await page.reload();
  await expect(page.getByRole('button', { name: '停止生成', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '下载预览', exact: true })).toBeEnabled();
  expect(await page.locator('.message.user').count()).toBe(1);
  await expect(page.getByRole('combobox', { name: '切换视频版本' }).locator('option')).toHaveCount(
    1,
  );
});

// 等待超过原任务完成时间，确认停止后的页面不会突然出现迟到成片。
test('cancelled generation stays cancelled after the original completion time', async ({
  page,
}) => {
  await startFresh(page);
  await page.getByRole('button', { name: '停止生成', exact: true }).click();
  await expect(page.getByText('本次制作已停止。', { exact: false })).toBeVisible();
  // 此处特意等过演示任务的十秒完成点，用来验证取消竞态，而不是等待页面控件加载。
  await page.waitForTimeout(11000);
  await expect(page.getByRole('button', { name: '下载预览', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: '继续这次制作', exact: true })).toBeVisible();
});

// 先检查讨论无副作用，再检查修改生成新版以及历史快照的只读保护。
test('discussion does not create a new version; revision preserves the historical snapshot', async ({
  page,
}) => {
  await page.goto('/studio.html#/workspace');
  await page.getByRole('button', { name: '仅讨论', exact: true }).click();
  await page.getByRole('textbox', { name: '视频创作需求' }).fill('如何安排开头？');
  await page.getByRole('button', { name: '发送消息', exact: true }).click();
  await expect(
    page.getByText('当前是仅讨论模式，不会修改或生成视频。', { exact: false }),
  ).toBeVisible();
  await expect(page.getByRole('combobox', { name: '切换视频版本' }).locator('option')).toHaveCount(
    1,
  );
  await page.getByRole('button', { name: '视频创作', exact: true }).click();
  await page
    .getByRole('textbox', { name: '视频创作需求' })
    .fill('第二个镜头换成咖啡特写，配音慢一点');
  await page.getByRole('button', { name: '修改视频', exact: true }).click();
  await expect(page.getByRole('combobox', { name: '切换视频版本' }).locator('option')).toHaveCount(
    2,
  );
  const versions = page.getByRole('combobox', { name: '切换视频版本' });
  await versions.selectOption({ label: 'v1 历史版' });
  await expect(page.getByRole('textbox', { name: '视频创作需求' })).toBeDisabled();
  await page.getByRole('tab', { name: '音频', exact: true }).click();
  await expect(page.getByRole('combobox', { name: '配音音色' })).toBeDisabled();
  await page.getByRole('button', { name: '回到当前草稿', exact: true }).first().click();
  await expect(page.getByRole('textbox', { name: '视频创作需求' })).toBeEnabled();
});

// 编辑与生成是两步：先出现“尚未生成”提示，再由更新动作产生带新脚本的成片。
test('manual script save marks the draft dirty and regenerates storyboard without erasing text', async ({
  page,
}) => {
  await page.goto('/studio.html#/workspace');
  // 改版后脚本位于可收起的详情面板，先从画布工具栏展开。
  await page.getByRole('tab', { name: '脚本', exact: true }).click();
  await page.getByRole('button', { name: '编辑', exact: true }).click();
  await page.getByRole('textbox', { name: '作品标题', exact: true }).fill('给清晨一点温柔');
  await page
    .getByRole('textbox', { name: '解说文案', exact: true })
    .fill('清晨的第一缕光。\n和刚刚好的咖啡香。');
  await page.getByRole('button', { name: '保存修改', exact: true }).click();
  await expect(page.getByText('有修改尚未生成')).toBeVisible();
  await page.getByRole('button', { name: '更新视频', exact: true }).click();
  await expect(page.getByRole('combobox', { name: '切换视频版本' }).locator('option')).toHaveCount(
    2,
  );
  await expect(page.locator('.preview-caption')).toContainText('给清晨一点温柔');
  await expect(page.locator('.shot-card')).toHaveCount(2);
});

// 上传真实图片后刷新验证文件恢复，再用伪造 PNG 内容验证实际解码校验。
test('upload persists across reload and rejects a corrupt image', async ({ page }) => {
  await page.goto('/studio.html#/create');
  await page.locator('input[type=file]').setInputFiles(resolve('public/media/coffee-cup.jpg'));
  await expect(page.locator('.attachment-list')).toContainText('coffee-cup.jpg');
  await page.getByRole('tab', { name: '素材', exact: true }).click();
  await expect(page.locator('.asset-card')).toHaveCount(1);
  await page.reload();
  await page.getByRole('tab', { name: '素材', exact: true }).click();
  await expect(page.locator('.asset-card img')).toBeVisible();
  await expect
    .poll(() =>
      page
        .locator('.asset-card img')
        .evaluate((element) => (element as HTMLImageElement).naturalWidth),
    )
    .toBeGreaterThan(0);
  await page.locator('input[type=file]').setInputFiles({
    name: 'broken.png',
    mimeType: 'image/png',
    buffer: Buffer.from('invalid image data'),
  });
  await expect(page.getByRole('status')).toContainText('这张图片无法读取');
  await expect(page.locator('.asset-card')).toHaveCount(1);
});

// 从帮助面板覆盖失败重试与补充信息两条异常恢复路径。
test('failed task can retry and a clarification can continue the original task', async ({
  page,
}) => {
  await page.goto('/studio.html#/workspace');
  await page.getByRole('button', { name: '查看使用指南' }).click();
  await page.getByRole('button', { name: '失败后继续制作' }).click();
  await expect(page.getByRole('button', { name: '从中断处重试', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '从中断处重试', exact: true }).click();
  await expect(page.getByRole('button', { name: '下载预览', exact: true })).toBeEnabled();
  expect(await page.locator('.message.user').count()).toBe(1);
  await page.getByRole('button', { name: '查看使用指南' }).click();
  await page.getByRole('button', { name: '补充信息后继续' }).click();
  await page.getByRole('textbox', { name: '补充信息' }).fill('暖日咖啡');
  await page.getByRole('button', { name: '继续制作', exact: true }).click();
  await expect(page.getByRole('button', { name: '下载预览', exact: true })).toBeEnabled();
  await expect(page.getByRole('combobox', { name: '切换视频版本' }).locator('option')).toHaveCount(
    1,
  );
});

// 在手机尺寸下完成生成、作品查看和返回新建，并检查没有横向溢出。
test('mobile workspace switches panels and completes a creation without horizontal overflow', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/studio.html#/create');
  await page.getByRole('textbox', { name: '视频创作需求' }).fill('做一个电影感的山野旅行短片');
  await page.getByRole('button', { name: '生成视频', exact: true }).click();
  await page.getByRole('button', { name: '我的作品', exact: false }).click();
  await expect(page.getByRole('button', { name: '下载预览', exact: true })).toBeEnabled();
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false);
  await page.screenshot({ path: 'docs/screenshots/mobile.png', fullPage: true });
  await page.getByRole('button', { name: '打开项目导航' }).click();
  await expect(page.getByRole('button', { name: '新建创作', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '新建创作', exact: true }).click();
  await expect(page.getByRole('heading', { name: /把灵感，.*变成看得见的故事/ })).toBeVisible();
});

// 保存配置不能覆盖旧视频；同时检查改名持久化、确认删除和首页落点恢复。
test('settings save keeps the old video, rename persists, and confirmed deletion returns to creation', async ({
  page,
}) => {
  await page.goto('/studio.html#/workspace');
  const videoSource = await page.locator('video').getAttribute('src');
  await page.getByRole('tab', { name: '设置', exact: true }).click();
  await page.getByRole('switch', { name: '自动字幕' }).click();
  await page.getByRole('button', { name: '保存修改', exact: true }).click();
  await expect(page.getByText('有修改尚未生成')).toBeVisible();
  expect(await page.locator('video').getAttribute('src')).toBe(videoSource);
  await page.getByRole('button', { name: '更多项目操作' }).click();
  await page.getByRole('button', { name: '重命名项目', exact: true }).click();
  await page.getByRole('textbox', { name: '项目名称', exact: true }).fill('周末咖啡创作');
  await page.getByRole('button', { name: '保存名称', exact: true }).click();
  await expect(page.locator('.project-title')).toHaveText('周末咖啡创作');
  await page.reload();
  await expect(page.locator('.project-title')).toHaveText('周末咖啡创作');
  await page.getByRole('button', { name: '更多项目操作' }).click();
  await page.getByRole('button', { name: '删除项目', exact: true }).click();
  await page.getByRole('button', { name: '确认删除', exact: true }).click();
  await expect(page).toHaveURL('/studio.html#/create');
  await page.goto('/studio.html#/workspace');
  await expect(page.locator('.project-title')).not.toHaveText('周末咖啡创作');
  await expect(page.getByRole('textbox', { name: '视频创作需求' })).toBeVisible();
});

// 模板只填输入框；任务运行中切换项目后，原项目结果不能串到当前作品。
test('templates only fill the composer, and switching projects cannot receive another project result', async ({
  page,
}) => {
  await page.goto('/studio.html#/workspace');
  await page.getByRole('button', { name: '灵感模板', exact: false }).click();
  await page.getByRole('dialog').getByRole('button', { name: '旅行记录', exact: false }).click();
  await expect(page).toHaveURL('/studio.html#/create');
  await expect(page.getByRole('textbox', { name: '视频创作需求' })).toHaveValue(/山野旅行/);
  await expect(page.getByRole('button', { name: '停止生成', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '生成视频', exact: true }).click();
  await expect(page).toHaveURL(/\/projects\/prj_/);
  const sourceUrl = page.url();
  await page.getByRole('button', { name: '随行水杯产品短片 已完成', exact: true }).click();
  await expect(page.locator('.project-title')).toHaveText('随行水杯产品短片');
  await page.waitForTimeout(11000);
  await expect(page.locator('.project-title')).toHaveText('随行水杯产品短片');
  await expect(page.locator('.preview-caption')).toContainText('把日常，装进美好');
  await page.goto(sourceUrl);
  await expect(page.getByRole('button', { name: '下载预览', exact: true })).toBeEnabled();
  await expect(page.locator('.preview-caption')).toContainText('去山野，找回自己');
});

// 时间轴定位真实成片；缩放、静音、素材定位和切项目后的播放头都需要与 video 同步。
test('timeline seeks the real player, zooms, mutes and resets on project change', async ({
  page,
}) => {
  await page.goto('/studio.html#/workspace');
  const video = page.locator('video');
  await expect.poll(() => video.evaluate((element) => element.readyState)).toBeGreaterThan(0);
  await page.getByRole('button', { name: '跳转到镜头 2', exact: true }).click();
  await expect.poll(() => video.evaluate((element) => element.currentTime)).toBe(7);
  await expect(page.locator('.playhead')).toHaveAttribute('style', /23\.33/);
  await page.getByRole('slider', { name: '画布播放进度', exact: true }).fill('15');
  await expect.poll(() => video.evaluate((element) => element.currentTime)).toBe(15);
  await page.getByRole('slider', { name: '时间轴缩放', exact: true }).fill('2');
  await expect(page.locator('.timeline-sequence')).toHaveAttribute('style', /200%/);
  await page.getByRole('button', { name: '视频静音', exact: true }).click();
  await expect.poll(() => video.evaluate((element) => element.muted)).toBe(true);
  await page.getByRole('button', { name: '定位镜头 3', exact: true }).click();
  await expect.poll(() => video.evaluate((element) => element.currentTime)).toBe(15);
  await page.getByRole('button', { name: '随行水杯产品短片 已完成', exact: true }).click();
  await expect(page.locator('.project-title')).toHaveText('随行水杯产品短片');
  await expect.poll(() => video.evaluate((element) => element.currentTime)).toBe(0);
});

// 撤销/重做只作用于未保存草稿，切换项目必须清理编辑历史；比例修改也不能偷偷替换成片。
test('draft undo and redo preserve the current video and clear on project switch', async ({
  page,
}) => {
  await page.goto('/studio.html#/workspace');
  const source = await page.locator('video').getAttribute('src');
  await expect(page.getByRole('button', { name: '撤销草稿编辑' })).toBeDisabled();
  await page.getByRole('combobox', { name: '画布比例设置' }).selectOption('16:9');
  const settingRatio = page.locator('.ratio-options button', { hasText: '16:9' });
  await expect(settingRatio).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: '撤销草稿编辑' }).click();
  await expect(page.locator('.ratio-options button', { hasText: '9:16' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await page.getByRole('button', { name: '重做草稿编辑' }).click();
  await expect(settingRatio).toHaveAttribute('aria-pressed', 'true');
  expect(await page.locator('video').getAttribute('src')).toBe(source);
  await page.getByRole('button', { name: '保存修改', exact: true }).click();
  await expect(page.getByText('有修改尚未生成')).toBeVisible();
  await expect(page.getByRole('button', { name: '撤销草稿编辑' })).toBeDisabled();
  await page.getByRole('button', { name: '随行水杯产品短片 已完成', exact: true }).click();
  await expect(page.getByRole('button', { name: '重做草稿编辑' })).toBeDisabled();
});

// 逐档检查分区布局，手机端仍可展开设置、播放与导出，所有屏幕都不能产生横向滚动。
test('workbench stays within phone, tablet and desktop viewports', async ({ page }) => {
  await page.goto('/studio.html#/workspace');
  for (const width of [390, 768, 1024, 1280, 1920]) {
    await page.setViewportSize({ width, height: 900 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(
      false,
    );
    if (width <= 1050 && width >= 768) {
      const toggle = page.getByRole('button', { name: '打开作品预览' });
      if ((await toggle.getAttribute('aria-expanded')) !== 'true') await toggle.click();
    } else if (width < 768) {
      await page.getByRole('button', { name: '我的作品', exact: false }).click();
    }
    await expect(page.locator('video')).toBeVisible();
    const bounds = await page.locator('video').boundingBox();
    expect(bounds!.width).toBeGreaterThan(60);
    expect(bounds!.width / bounds!.height).toBeCloseTo(9 / 16, 2);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(width);
    if (width >= 768 && width <= 1050) {
      await page.getByRole('button', { name: '打开作品预览' }).click();
      await expect(page.getByRole('textbox', { name: '视频创作需求' })).toBeVisible();
    }
  }
});

// 横屏和方形必须在不同窗口中保持真实比例，不能因最大高度限制而裁切或拉伸。
test('landscape and square generations retain their actual canvas aspect ratio', async ({
  page,
}) => {
  await page.goto('/studio.html#/workspace');
  for (const [ratio, width, height] of [
    ['16:9', 1280, 720],
    ['1:1', 720, 720],
  ] as const) {
    await page.getByRole('combobox', { name: '画布比例设置' }).selectOption(ratio);
    await page.getByRole('button', { name: '保存修改', exact: true }).click();
    await page.getByRole('button', { name: '更新视频', exact: true }).click();
    await expect
      .poll(() =>
        page
          .locator('video')
          .evaluate((element) => ({ width: element.videoWidth, height: element.videoHeight })),
      )
      .toEqual({ width, height });
    await page.getByRole('button', { name: '收起作品详情', exact: true }).click();
    for (const viewport of [
      { width: 1440, height: 960 },
      { width: 1280, height: 720 },
    ]) {
      await page.setViewportSize(viewport);
      const bounds = await page.locator('video').boundingBox();
      expect(bounds!.width / bounds!.height).toBeCloseTo(width / height, 2);
      expect(bounds!.y + bounds!.height).toBeLessThan(viewport.height);
    }
  }
});
