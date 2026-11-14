import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

/** 独立官网构建：不包含 Tauri 工作台、接口层或任何后端。 */
export default defineConfig({
  base: process.env.VITE_BASE ?? "/",
  // 官网自包含：静态素材放在 website/public，不再依赖仓库根的 shared 目录。
  publicDir: "public",
  plugins: [vue()],
  server: { port: 5174, strictPort: true },
  preview: { port: 4174, strictPort: true },
  build: { outDir: "dist", emptyOutDir: true },
});
