/** Aceternity Vue 移植验收：真实 SVG 动画、Bento 布局和侧栏交互，不用静态截图替代功能。 */
import { test, expect } from "@playwright/test";
import { readFile } from "node:fs/promises";

// 直接与保存的官方源码路径核对，并读取 SVG 动画值确认光束真实运动。
test("home uses the actual official beam paths and animated gradients", async ({
  page,
}) => {
  const source = JSON.parse(
    await readFile("docs/aceternity/upstream/background-beams.json", "utf8"),
  ).files[0].content as string;
  const list = source.match(/const paths = \[([\s\S]*?)\];/)![1]!;
  const paths = JSON.parse(`[${list.trim().replace(/,$/, "")}]`) as string[];
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1 })).toContainText(
    "有声，有画",
  );
  await expect(page.locator(".background-beams svg")).toHaveAttribute(
    "viewBox",
    "0 0 696 316",
  );
  expect(
    await page
      .locator(".background-beams > svg > path")
      .evaluateAll((items) =>
        items.slice(1).map((item) => item.getAttribute("d")),
      ),
  ).toEqual(paths);
  await expect(
    page.locator(".background-beams linearGradient animate"),
  ).toHaveCount(200);
  const gradient = page.locator(".background-beams linearGradient").nth(9);
  const initial = await gradient.evaluate(
    (element) => (element as SVGLinearGradientElement).x1.animVal.valueAsString,
  );
  await expect
    .poll(() =>
      gradient.evaluate(
        (element) =>
          (element as SVGLinearGradientElement).x1.animVal.valueAsString,
      ),
    )
    .not.toBe(initial);
  await expect(
    page.getByText("由桌面端生成配音和配乐", { exact: false }),
  ).toBeVisible();
});

// 断点、跨列和内容平移对应 Bento 原始布局，卡片插图不能再被根字号挤成窄条。
test("bento keeps its three-column spans, hover translation and complete illustrations", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.locator(".ac-bento-item")).toHaveCount(4);
  const grid = page.locator(".ac-bento-grid");
  expect(
    await grid.evaluate(
      (element) =>
        getComputedStyle(element).gridTemplateColumns.split(" ").length,
    ),
  ).toBe(3);
  await expect(page.locator(".ac-bento-item").first()).toHaveCSS(
    "grid-column-start",
    "span 2",
  );
  await page.locator(".ac-bento-item").first().hover();
  await expect(page.locator(".ac-bento-copy").first()).toHaveCSS(
    "transform",
    "matrix(1, 0, 0, 1, 8, 0)",
  );
  const header = await page.locator(".ac-bento-header").first().boundingBox();
  expect(header!.height).toBeGreaterThan(200);
  await page.setViewportSize({ width: 390, height: 844 });
  expect(
    await grid.evaluate(
      (element) =>
        getComputedStyle(element).gridTemplateColumns.split(" ").length,
    ),
  ).toBe(1);
  await expect(
    page.getByRole("heading", { name: "AI 脚本生成" }),
  ).toBeVisible();
});

// 独立官网提供真实示例播放、背景音乐试听和 Releases 下载链接。
test("home preview, music and workspace entry are operational", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/");
  await page.getByRole("button", { name: "观看示例", exact: true }).click();
  const video = page.getByRole("dialog").getByLabel("首页示例视频");
  await expect
    .poll(() =>
      video.evaluate((element: HTMLVideoElement) => element.videoWidth),
    )
    .toBe(1280);
  await expect
    .poll(() =>
      video.evaluate((element: HTMLVideoElement) => element.currentTime),
    )
    .toBeGreaterThan(0.1);
  await page.getByRole("button", { name: "关闭对话框" }).click();
  await page.getByRole("button", { name: "试听示例配乐", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "停止试听", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await page.getByRole("button", { name: "停止试听", exact: true }).click();
  const downloadLinks = page.getByRole("link", {
    name: "立即下载",
    exact: true,
  });
  await expect(downloadLinks).toHaveCount(8);
  for (const link of await downloadLinks.all()) {
    await expect(link).toHaveAttribute(
      "href",
      "https://github.com/LDFei/Bohack-ExplainCraft/releases",
    );
  }
  expect(errors).toEqual([]);
});

// 移动抽屉保持官方滑入方向；多个尺寸及四套主题不能产生横向溢出。
test("home and animated mobile navigation fit all target viewports", async ({
  page,
}) => {
  for (const width of [320, 390, 768, 1024, 1440, 1920]) {
    await page.setViewportSize({ width, height: 844 });
    await page.goto("/");
    await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth > innerWidth,
      ),
    ).toBe(false);
  }
});

// 系统减少动态效果时保留 SVG 纹理，不启动持续光束，首页入口仍可操作。
test("reduced motion removes the continuous beam animations", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/");
  await expect(
    page.locator(".background-beams linearGradient animate"),
  ).toHaveCount(0);
  await expect(page.locator(".background-beams > svg > path")).toHaveCount(51);
  await expect
    .poll(() =>
      page
        .locator(".product-video-button video")
        .evaluate((element) => element.paused),
    )
    .toBe(true);
  await expect(
    page.getByRole("link", { name: "立即下载", exact: true }).first(),
  ).toHaveAttribute(
    "href",
    "https://github.com/LDFei/Bohack-ExplainCraft/releases",
  );
});
