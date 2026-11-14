/**
 * 官网产物边界检查：只允许静态页面、样式、脚本与媒体，不能混入工作台接口和本机演示数据库。
 */
import { readFile, readdir } from "node:fs/promises";

const websiteRoot = new URL("../dist/", import.meta.url);
const forbiddenMarkers = [
  "__EXPLAINCRAFT_API__",
  "/api/v1",
  "lingjing-studio-v1",
  "lingjing-media",
  "class HttpApi",
  "class DemoApi",
];

/** 递归取得构建产物路径，使用 URL 保持 Windows 与 Unix 行为一致。 */
async function filesUnder(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = await Promise.all(
    entries.map((entry) => {
      const path = new URL(entry.name, directory);
      return entry.isDirectory()
        ? filesUnder(new URL(`${entry.name}/`, directory))
        : [path];
    }),
  );
  return files.flat();
}

const files = await filesUnder(websiteRoot);
if (files.some((path) => path.pathname.endsWith("/studio.html"))) {
  throw new Error("官网产物不能包含 Tauri 工作台 studio.html。");
}
if (!files.some((path) => path.pathname.endsWith("/index.html"))) {
  throw new Error("官网产物缺少 index.html。");
}

for (const path of files.filter((path) =>
  /\.(?:html|css|js)$/.test(path.pathname),
)) {
  const content = await readFile(path, "utf8");
  const marker = forbiddenMarkers.find((marker) => content.includes(marker));
  if (marker)
    throw new Error(`官网静态产物 ${path.pathname} 包含后端标记：${marker}`);
}

console.log(
  `官网静态产物检查通过：${files.length} 个文件，无工作台或后端接口。`,
);
