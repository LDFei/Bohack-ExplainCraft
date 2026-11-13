/**
 * 浏览器回归配置。路径相对 front/，测试需要时启动当前前端服务，结束后关闭自建服务。
 */
import { defineConfig } from '@playwright/test';

export default defineConfig({
  // 路径以 front/ 为起点，测试代码和测试数据跟随前端一起移动。
  testDir: './tests/e2e',
  timeout: 45000,
  expect: { timeout: 15000 },
  fullyParallel: true,
  workers: 2,
  // 每个测试使用隔离的浏览器上下文，不修改日常浏览器里的演示项目数据。
  use: {
    baseURL: 'http://127.0.0.1:5173',
    channel: 'chrome',
    headless: true,
    viewport: { width: 1440, height: 960 },
    locale: 'zh-CN',
    timezoneId: 'Asia/Shanghai',
    // 只在失败时保留追踪和截图，正常回归不留下大量临时产物。
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  // 没有可用服务时自动启动；已有同端口服务时复用，避免测试另起一个不同地址。
  webServer: {
    command: 'npm run dev',
    url: 'http://127.0.0.1:5173',
    reuseExistingServer: true,
    timeout: 15000,
  },
});
