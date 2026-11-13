/** 外观回归：只验证用户偏好和工作台状态隔离，不向真实服务发送请求。 */
import { test, expect } from '@playwright/test';
const preferenceKey = 'explaincraft.ui-theme';

// 四套预设都要覆盖实际面板，不能只更新选择卡片；切换也不能重建播放器或清空输入。
test('four complete themes preserve the live project, player and unsent input', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/workspace');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'silver');
  const video = page.locator('video');
  await expect.poll(() => video.evaluate((element) => element.readyState)).toBeGreaterThan(0);
  const source = await video.getAttribute('src');
  const originalElement = await video.elementHandle();
  const projectUrl = page.url();
  await page.getByRole('textbox', { name: '视频创作需求' }).fill('保留这条尚未发送的创作想法');
  await page.getByRole('slider', { name: '画布播放进度', exact: true }).fill('12');
  await page.getByRole('button', { name: '更换主题' }).click();
  for (const [name, id, background] of [
    ['浅丁香创意空间', 'lilac', 'rgb(252, 250, 255)'],
    ['暖纸编辑室', 'paper', 'rgb(252, 250, 245)'],
    ['雾蓝创作台', 'mist', 'rgb(249, 252, 255)'],
    ['银灰工作室', 'silver', 'rgb(249, 250, 252)'],
  ]) {
    await page.getByRole('radio', { name, exact: true }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', id!);
    await expect(page.locator('.conversation')).toHaveCSS('background-color', background!);
    expect(await page.evaluate((key) => localStorage.getItem(key), preferenceKey)).toBe(id);
    expect(await video.getAttribute('src')).toBe(source);
    expect(
      await originalElement!.evaluate((element) =>
        element.isSameNode(document.querySelector('video')),
      ),
    ).toBe(true);
    expect(await video.evaluate((element) => element.currentTime)).toBe(12);
    expect(page.url()).toBe(projectUrl);
  }
  await page.getByRole('button', { name: '完成', exact: true }).click();
  await expect(page.getByRole('textbox', { name: '视频创作需求' })).toHaveValue(
    '保留这条尚未发送的创作想法',
  );
  // 实际播放中再次切换，确认没有暂停、回到起点或替换 video 元素。
  await page.getByRole('button', { name: '播放或暂停视频', exact: true }).click();
  await expect.poll(() => video.evaluate((element) => element.currentTime)).toBeGreaterThan(12.1);
  await page.getByRole('button', { name: '更换主题' }).click();
  await page.getByRole('radio', { name: '暖纸编辑室', exact: true }).click();
  await page.getByRole('button', { name: '完成', exact: true }).click();
  await expect.poll(() => video.evaluate((element) => element.paused)).toBe(false);
  expect(await video.getAttribute('src')).toBe(source);
  expect(
    await originalElement!.evaluate((element) =>
      element.isSameNode(document.querySelector('video')),
    ),
  ).toBe(true);
  await page.getByRole('button', { name: '暂停视频', exact: true }).click();
  expect(errors).toEqual([]);
});

// 刷新与更换项目都保留外观，恢复默认仅影响主题，不能删除项目记录。
test('theme preference survives refresh and project changes and can reset', async ({ page }) => {
  await page.goto('/workspace');
  const projects = await page.locator('.history-project').count();
  await page.getByRole('button', { name: '更换主题' }).click();
  await page.getByRole('radio', { name: '浅丁香创意空间' }).click();
  await page.getByRole('button', { name: '完成', exact: true }).click();
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'lilac');
  await page.getByRole('button', { name: '随行水杯产品短片 已完成', exact: true }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'lilac');
  await page.getByRole('button', { name: '更换主题' }).click();
  await expect(page.getByRole('radio', { name: '浅丁香创意空间' })).toBeFocused();
  await page.getByRole('button', { name: '恢复默认', exact: true }).click();
  await expect(page.getByRole('radio', { name: '银灰工作室' })).toHaveAttribute(
    'aria-checked',
    'true',
  );
  await page.getByRole('button', { name: '完成', exact: true }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'silver');
  await expect(page.locator('.project-title')).toHaveText('随行水杯产品短片');
  await expect(page.locator('.history-project')).toHaveCount(projects);
});

// 损坏的偏好要安全回退；仅阻止主题保存时，项目仍可正常加载且即时换肤仍然有效。
test('invalid or unwritable theme preferences never block the workspace', async ({ page }) => {
  await page.addInitScript((key) => {
    localStorage.setItem(key, 'unsupported-theme');
    const original = Storage.prototype.setItem;
    Storage.prototype.setItem = function (name: string, value: string) {
      if (name === key) throw new DOMException('Storage unavailable', 'QuotaExceededError');
      return original.call(this, name, value);
    };
  }, preferenceKey);
  await page.goto('/workspace');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'silver');
  await expect(page.locator('video')).toBeVisible();
  await page.getByRole('button', { name: '更换主题' }).click();
  await page.getByRole('radio', { name: '雾蓝创作台' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'mist');
  await expect(page.getByRole('dialog').getByRole('status')).toContainText('未允许保存');
  await page.getByRole('button', { name: '完成', exact: true }).click();
  await page.reload();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'silver');
  await expect(page.getByRole('textbox', { name: '视频创作需求' })).toBeEnabled();
});

// 同源页面同步偏好，只更新另一页外观，不能切换其项目。
test('theme preference synchronizes across open tabs', async ({ page, context }) => {
  await page.goto('/workspace');
  const otherPage = await context.newPage();
  await otherPage.goto('/projects/demo-product');
  await expect(otherPage.locator('.project-title')).toHaveText('随行水杯产品短片');
  await page.getByRole('button', { name: '更换主题' }).click();
  await page.getByRole('radio', { name: '暖纸编辑室' }).click();
  await expect(otherPage.locator('html')).toHaveAttribute('data-theme', 'paper');
  await expect(otherPage.locator('.project-title')).toHaveText('随行水杯产品短片');
});

// 手机与平板均保留主题入口；方向键、焦点恢复和弹窗范围不能依赖桌面宽度。
test('theme picker supports keyboard and small screens without overflow', async ({ page }) => {
  await page.goto('/workspace');
  for (const width of [320, 390, 768, 1024, 1440]) {
    await page.setViewportSize({ width, height: 844 });
    await page.getByRole('button', { name: '更换主题' }).click();
    const silver = page.getByRole('radio', { name: '银灰工作室' });
    await silver.focus();
    await page.keyboard.press('End');
    await expect(page.getByRole('radio', { name: '雾蓝创作台' })).toBeFocused();
    await page.keyboard.press('ArrowLeft');
    await expect(page.getByRole('radio', { name: '暖纸编辑室' })).toBeFocused();
    await page.keyboard.press('Home');
    await expect(silver).toBeFocused();
    const bounds = await page.getByRole('dialog').boundingBox();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(width);
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(
      false,
    );
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog')).toHaveCount(0);
    await expect(page.getByRole('button', { name: '更换主题' })).toBeFocused();
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(
      false,
    );
  }
});

// 制作过程中换肤不重启任务、重复消息或丢失生成进度。
test('theme switching during generation keeps the original task running', async ({ page }) => {
  await page.goto('/create');
  await page.getByRole('textbox', { name: '视频创作需求' }).fill('做一个温暖的咖啡店宣传视频');
  await page.getByRole('button', { name: '生成视频', exact: true }).click();
  await expect(page.getByRole('button', { name: '停止生成', exact: true })).toBeVisible();
  const projectUrl = page.url();
  await page.getByRole('button', { name: '更换主题' }).click();
  await page.getByRole('radio', { name: '雾蓝创作台' }).click();
  await page.getByRole('button', { name: '完成', exact: true }).click();
  await expect(page.getByRole('button', { name: '下载预览', exact: true })).toBeEnabled();
  expect(page.url()).toBe(projectUrl);
  await expect(page.locator('.message.user')).toHaveCount(1);
  await expect(page.getByRole('combobox', { name: '切换视频版本' }).locator('option')).toHaveCount(
    1,
  );
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'mist');
});
