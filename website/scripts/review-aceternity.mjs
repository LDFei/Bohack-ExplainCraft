/** 在真实 Chrome 中保存一轮截图和布局指标；仅操作隔离的演示上下文。 */
import { chromium } from "@playwright/test";
import { mkdir, writeFile } from "node:fs/promises";
const round = process.argv[2] ?? "1";
const directory = `docs/screenshots/aceternity/round-${round}`;
await mkdir(directory, { recursive: true });
const browser = await chromium.launch({ channel: "chrome", headless: true });
const errors = [],
  results = [];
try {
  const page = await browser.newPage({
    viewport: { width: 1440, height: 960 },
    locale: "zh-CN",
  });
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("http://127.0.0.1:5174/");
  await page.getByRole("heading", { level: 1 }).waitFor();
  // 内置视频开头有淡入，等待实际帧进入示例画面后再做视觉审查。
  await page.waitForFunction(
    () =>
      document.querySelector(".product-video-button video")?.currentTime > 1,
  );
  await page.screenshot({
    path: `${directory}/home-desktop.png`,
    fullPage: true,
  });
  results.push(
    await page.evaluate(() => ({
      view: "home-desktop",
      width: innerWidth,
      overflow: document.documentElement.scrollWidth > innerWidth,
      cards: [...document.querySelectorAll(".ac-bento-item")].map((item) => ({
        height: item.clientHeight,
        header: item.querySelector(".ac-bento-header").clientHeight,
        illustration:
          item.querySelector(".ac-bento-header").firstElementChild.scrollHeight,
      })),
      beamPaths: document.querySelectorAll(".background-beams path").length,
      animatedGradients: document.querySelectorAll(
        ".background-beams linearGradient animate",
      ).length,
    })),
  );
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: `${directory}/home-mobile.png`,
    fullPage: true,
  });
  results.push(
    await page.evaluate(() => ({
      view: "home-mobile",
      width: innerWidth,
      overflow: document.documentElement.scrollWidth > innerWidth,
    })),
  );
  await writeFile(
    `${directory}/metrics.json`,
    JSON.stringify({ round, results, errors }, null, 2),
  );
  console.log(JSON.stringify({ round, results, errors }));
} finally {
  await browser.close();
}
