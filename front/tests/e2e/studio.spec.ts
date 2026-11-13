import { test, expect } from '@playwright/test';
import type { Page } from '@playwright/test';
import { resolve } from 'node:path';
import { readFile } from 'node:fs/promises';

async function startFresh(page: Page, text = '做一个温暖的咖啡店宣传视频') {
  await page.goto('/create');
  await page.getByRole('textbox', { name: '视频创作需求' }).fill(text);
  await page.getByRole('button', { name: '生成视频', exact: true }).click();
  await expect(page).toHaveURL(/\/projects\/prj_/);
}

test('initial workspace plays a real local video and downloads a valid MP4', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/');
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

test('cancelled generation stays cancelled after the original completion time', async ({
  page,
}) => {
  await startFresh(page);
  await page.getByRole('button', { name: '停止生成', exact: true }).click();
  await expect(page.getByText('本次制作已停止。', { exact: false })).toBeVisible();
  await page.waitForTimeout(11000);
  await expect(page.getByRole('button', { name: '下载预览', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: '继续这次制作', exact: true })).toBeVisible();
});

test('discussion does not create a new version; revision preserves the historical snapshot', async ({
  page,
}) => {
  await page.goto('/');
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

test('manual script save marks the draft dirty and regenerates storyboard without erasing text', async ({
  page,
}) => {
  await page.goto('/');
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

test('upload persists across reload and rejects a corrupt image', async ({ page }) => {
  await page.goto('/create');
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

test('failed task can retry and a clarification can continue the original task', async ({
  page,
}) => {
  await page.goto('/');
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

test('mobile workspace switches panels and completes a creation without horizontal overflow', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/create');
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

test('settings save keeps the old video, rename persists, and confirmed deletion returns to creation', async ({
  page,
}) => {
  await page.goto('/');
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
  await expect(page).toHaveURL('/create');
  await page.goto('/');
  await expect(page.locator('.project-title')).not.toHaveText('周末咖啡创作');
  await expect(page.getByRole('textbox', { name: '视频创作需求' })).toBeVisible();
});

test('templates only fill the composer, and switching projects cannot receive another project result', async ({
  page,
}) => {
  await page.goto('/');
  await page.getByRole('button', { name: '灵感模板', exact: false }).click();
  await page.getByRole('dialog').getByRole('button', { name: '旅行记录', exact: false }).click();
  await expect(page).toHaveURL('/create');
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
