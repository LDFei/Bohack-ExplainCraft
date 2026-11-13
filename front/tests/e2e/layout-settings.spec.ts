/** 布局与通用设置只操作浏览器偏好；验证真实拖拽和数据隔离，不调用预留 API。 */
import { test, expect } from '@playwright/test';
import type { Page } from '@playwright/test';
const layoutKey = 'explaincraft.workspace-layout',
  settingsKey = 'explaincraft.app-settings';
/** 用真实指针拖动分隔线，覆盖指针捕获与连续移动。 */
async function resizeColumns(page: Page, delta: number) {
  const separator = page.locator('.pane-separator').nth(1);
  const box = (await separator.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + 130);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + delta, box.y + 130, { steps: 12 });
  await page.mouse.up();
}
/** 打开本机通用设置，不依赖项目菜单或演示帮助。 */
async function openSettings(page: Page) {
  await page.getByRole('button', { name: '打开工作台设置' }).click();
  await expect(page.getByRole('dialog', { name: '工作台设置' })).toBeVisible();
}

// 保留一个帮助入口，并让左下角设置替代头像，不再出现重复的指南菜单。
test('one help entry and a real settings button replace duplicate help and avatar', async ({
  page,
}) => {
  await page.goto('/workspace');
  await expect(page.getByRole('button', { name: '查看使用指南' })).toHaveCount(1);
  await expect(page.getByRole('button', { name: '使用帮助', exact: true })).toHaveCount(0);
  await expect(page.locator('.workspace-avatar')).toHaveCount(0);
  await page.getByRole('button', { name: '更多项目操作' }).click();
  await expect(page.locator('.toolbar-menu').getByText('使用指南', { exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '更多项目操作' }).click();
  await openSettings(page);
  await expect(page.getByRole('combobox', { name: '发送快捷键' })).toBeVisible();
});

// 扩大 Copilot 必须实际缩小画布，且不重建视频或丢失输入；刷新恢复实际宽度。
test('resizing grows Copilot, shrinks preview and persists without remounting the player', async ({
  page,
}) => {
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.goto('/workspace');
  const video = page.locator('.preview-panel video');
  await expect.poll(() => video.evaluate((element) => element.readyState)).toBeGreaterThan(0);
  const element = await video.elementHandle();
  const beforeChat = (await page.locator('.conversation').boundingBox())!.width;
  const beforePreview = (await page.locator('.preview-panel').boundingBox())!.width;
  await page.getByRole('textbox', { name: '视频创作需求' }).fill('这条尚未发送的创作需求应保留');
  await page.getByRole('slider', { name: '画布播放进度', exact: true }).fill('12');
  await resizeColumns(page, -180);
  await expect
    .poll(async () => (await page.locator('.conversation').boundingBox())!.width)
    .toBeGreaterThan(beforeChat + 150);
  expect((await page.locator('.preview-panel').boundingBox())!.width).toBeLessThan(
    beforePreview - 150,
  );
  expect(
    await element!.evaluate((node) =>
      node.isSameNode(document.querySelector('.preview-panel video')),
    ),
  ).toBe(true);
  expect(await video.evaluate((node) => node.currentTime)).toBe(12);
  await expect(page.getByRole('textbox', { name: '视频创作需求' })).toHaveValue(
    '这条尚未发送的创作需求应保留',
  );
  const afterChat = (await page.locator('.conversation').boundingBox())!.width;
  await page.reload();
  await expect
    .poll(async () => (await page.locator('.conversation').boundingBox())!.width)
    .toBeCloseTo(afterChat, 0);
  expect(errors).toEqual([]);
});

// 原生 HTML 拖放改变 CSS 列号；文件上传不会误进入面板拖动流程。
test('dragging panel handles reorders panels, supports keyboard moves and preserves DOM', async ({
  page,
}) => {
  await page.goto('/workspace');
  const video = await page.locator('.preview-panel video').elementHandle();
  await page.getByRole('textbox', { name: '视频创作需求' }).fill('面板换位不清空输入');
  await page
    .getByRole('button', { name: '拖动AI 对话面板' })
    .dragTo(page.locator('.preview-heading'));
  await expect
    .poll(
      async () =>
        (await page.locator('.conversation').boundingBox())!.x <
        (await page.locator('.preview-panel').boundingBox())!.x,
    )
    .toBe(true);
  await expect(page.locator('.drop-overlay')).toHaveCount(0);
  expect(
    await video!.evaluate((node) =>
      node.isSameNode(document.querySelector('.preview-panel video')),
    ),
  ).toBe(true);
  await expect(page.getByRole('textbox', { name: '视频创作需求' })).toHaveValue(
    '面板换位不清空输入',
  );
  await page.reload();
  await expect
    .poll(
      async () =>
        (await page.locator('.conversation').boundingBox())!.x <
        (await page.locator('.preview-panel').boundingBox())!.x,
    )
    .toBe(true);
  await page.getByRole('button', { name: '拖动AI 对话面板' }).press('Alt+ArrowRight');
  await expect
    .poll(
      async () =>
        (await page.locator('.conversation').boundingBox())!.x >
        (await page.locator('.preview-panel').boundingBox())!.x,
    )
    .toBe(true);
});

// 时间轴支持真实指针调高、键盘调节和刷新恢复；小窗口限制实际高度避免挤出控件。
test('timeline height resizes, persists and remains bounded in shorter windows', async ({
  page,
}) => {
  await page.goto('/workspace');
  const separator = page.getByRole('separator', { name: '调整时间轴高度' });
  const box = (await separator.boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2, box.y - 90, { steps: 10 });
  await page.mouse.up();
  await expect
    .poll(async () => (await page.locator('.timeline').boundingBox())!.height)
    .toBeGreaterThan(320);
  const height = (await page.locator('.timeline').boundingBox())!.height;
  await page.reload();
  await expect
    .poll(async () => (await page.locator('.timeline').boundingBox())!.height)
    .toBeCloseTo(height, 0);
  await page.getByRole('separator', { name: '调整时间轴高度' }).press('ArrowDown');
  await expect
    .poll(async () => (await page.locator('.timeline').boundingBox())!.height)
    .toBeLessThan(height - 10);
  await page.setViewportSize({ width: 1440, height: 620 });
  const bounds = (await page.locator('.preview-footer').boundingBox())!;
  expect(bounds.y + bounds.height).toBeLessThanOrEqual(620);
});

// 通用设置真正影响发送行为；关闭布局记忆后刷新回默认，项目内容仍在。
test('general settings save the shortcut, reset layout and can disable layout memory', async ({
  page,
}) => {
  await page.goto('/workspace');
  await resizeColumns(page, -150);
  await openSettings(page);
  await page.getByRole('button', { name: '恢复默认布局', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('布局已恢复默认');
  await page.getByRole('combobox', { name: '发送快捷键' }).selectOption('mod-enter');
  await page.getByRole('checkbox', { name: '记住工作台布局' }).uncheck();
  await page.getByRole('button', { name: '保存设置', exact: true }).click();
  await resizeColumns(page, -120);
  expect(await page.evaluate((key) => localStorage.getItem(key), layoutKey)).toBeNull();
  await page.goto('/create');
  await page.reload();
  await page.getByRole('button', { name: '仅讨论', exact: true }).click();
  const input = page.getByRole('textbox', { name: '视频创作需求' });
  await input.fill('如何安排开头？');
  await input.press('Enter');
  await expect(page).toHaveURL('/create');
  await expect(input).toHaveValue('如何安排开头？\n');
  await input.press('Control+Enter');
  await expect(page).toHaveURL(/\/projects\/prj_/);
  await expect(page.locator('.message.user')).toHaveCount(1);
});

// 四类模块、五个端点独立保存非敏感配置；密钥不进入浏览器持久化存储，预留配置不发请求。
test('four API modules reserve five connections and never persist keys or call providers', async ({
  page,
}) => {
  const providerRequests: string[] = [];
  page.on('request', (request) => {
    if (request.url().includes('provider.example.test')) providerRequests.push(request.url());
  });
  await page.goto('/workspace');
  await openSettings(page);
  await page.getByRole('tab', { name: 'API 配置', exact: false }).click();
  await expect(page.locator('.api-module')).toHaveCount(4);
  await page.getByLabel('文生图 API接口地址').fill('https://provider.example.test/text-to-image');
  await page.getByLabel('文生图 API模型名称').fill('image-model-demo');
  await page.getByLabel('文生图 API密钥').fill('test-session-key');
  await page
    .getByLabel('图生视频 API接口地址')
    .fill('https://provider.example.test/image-to-video');
  await page.getByLabel('图生视频 API模型名称').fill('video-model-demo');
  for (const [module, slot] of [
    ['背景音乐 API 模块', '背景音乐 API'],
    ['人声 API 模型', '人声 API'],
    ['视觉大语言 API 模块', '视觉大语言 API'],
  ]) {
    await page.locator('.api-module').filter({ hasText: module }).locator('summary').click();
    await page.getByLabel(`${slot}接口地址`).fill('https://provider.example.test/service');
    await page.getByLabel(`${slot}模型名称`).fill('reserved-model');
  }
  await page.getByRole('button', { name: '保存设置', exact: true }).click();
  const saved = await page.evaluate((key) => localStorage.getItem(key), settingsKey);
  expect(saved).toContain('image-model-demo');
  expect(saved).toContain('video-model-demo');
  expect(saved).not.toContain('test-session-key');
  expect(saved).not.toContain('apiKey');
  await openSettings(page);
  await page.getByRole('tab', { name: 'API 配置', exact: false }).click();
  await expect(page.getByLabel('文生图 API密钥')).toHaveValue('test-session-key');
  await page.getByRole('button', { name: '取消', exact: true }).click();
  await page.reload();
  await openSettings(page);
  await page.getByRole('tab', { name: 'API 配置', exact: false }).click();
  await expect(page.getByLabel('文生图 API模型名称')).toHaveValue('image-model-demo');
  await expect(page.getByLabel('文生图 API密钥')).toHaveValue('');
  expect(providerRequests).toEqual([]);
});

// 取消丢弃未保存内容；无效地址显示原因而非保存为“已接入”。
test('settings cancel discards changes and invalid API URLs keep the dialog open', async ({
  page,
}) => {
  await page.goto('/workspace');
  await openSettings(page);
  await page.getByRole('combobox', { name: '设置主题外观' }).selectOption('mist');
  await page.getByRole('button', { name: '取消', exact: true }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'silver');
  await openSettings(page);
  await page.getByRole('tab', { name: 'API 配置', exact: false }).click();
  await page.getByLabel('文生图 API接口地址').fill('file:///tmp/not-an-api');
  await page.getByRole('button', { name: '保存设置', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('HTTP 或 HTTPS');
  await expect(page.getByRole('dialog', { name: '工作台设置' })).toBeVisible();
  expect(await page.evaluate((key) => localStorage.getItem(key), settingsKey)).toBeNull();
});

// 损坏布局安全回退；手机不展示桌面拖动手柄，通用与 API 面板均可正常打开。
test('corrupt layout recovers and settings remain usable on mobile and tablet', async ({
  page,
}) => {
  await page.addInitScript(
    (key) => localStorage.setItem(key, '{"version":1,"order":["copilot","copilot","preview"]}'),
    layoutKey,
  );
  await page.goto('/workspace');
  expect((await page.locator('.sidebar').boundingBox())!.x).toBe(0);
  for (const width of [320, 390, 768, 1024]) {
    await page.setViewportSize({ width, height: 844 });
    if (width < 768) await page.getByRole('button', { name: '打开项目导航' }).click();
    await openSettings(page);
    await page.getByRole('tab', { name: 'API 配置', exact: false }).click();
    const box = (await page.getByRole('dialog').boundingBox())!;
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(width);
    await page.getByRole('button', { name: '取消', exact: true }).click();
    if (width < 768) await page.getByRole('button', { name: '关闭导航' }).click();
    await expect(page.locator('.panel-drag-handle')).toHaveCount(0);
    expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(
      false,
    );
  }
});
