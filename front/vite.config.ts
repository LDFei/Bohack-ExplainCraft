/**
 * 前端开发与构建配置；端口固定，避免启动失败时悄悄切到另一个地址。
 */
import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

// HTTP 模式联调时把 /api 转发到 Rust 后端（仓库根目录 back/，默认 127.0.0.1:8787）。
// 同源转发让页面不需要处理跨域，签名媒体地址也能直接被 <video>/<img> 加载。
const backend = process.env.BACKEND_URL || 'http://127.0.0.1:8787';

// Tauri CLI 构建桌面端时设置的平台与调试标记（浏览器开发时不存在）。
const tauriPlatform = process.env.TAURI_ENV_PLATFORM;
const tauriDebug = !!process.env.TAURI_ENV_DEBUG;

export default defineConfig({
  // GitHub Pages 之类的子路径部署通过 VITE_BASE 传入仓库前缀，本地构建保持根路径。
  base: process.env.VITE_BASE ?? '/',
  // 将 .vue 单文件组件编译成浏览器可以加载的模块。
  plugins: [vue()],
  // 桌面端开发时 Tauri 与 Vite 共用一个终端，不清屏以免挡住 Rust 编译错误。
  clearScreen: false,
  // TAURI_ENV_* 变量暴露给前端代码（平台、调试标记等）。
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  // strictPort 阻止端口被占用时自动换号；桌面端的 devUrl 固定指向 5173。
  server: {
    port: 5173,
    strictPort: true,
    proxy: { '/api': { target: backend } },
    // Rust 工程的编译产物很多，不需要 Vite 监视。
    watch: { ignored: ['**/src-tauri/**'] },
  },
  preview: { port: 4173, strictPort: true, proxy: { '/api': { target: backend } } },
  build: {
    // Windows 桌面端使用 WebView2（Chromium 内核）；浏览器构建保持 Vite 默认目标。
    ...(tauriPlatform === 'windows' ? { target: 'chrome105' } : {}),
    // 桌面端调试构建保留可读代码与 sourcemap。
    minify: !tauriDebug,
    sourcemap: tauriDebug,
    // 封面与工作区各自产出 HTML：index.html 只含首页，studio.html 才是工作台。
    rollupOptions: {
      input: {
        cover: fileURLToPath(new URL('./index.html', import.meta.url)),
        studio: fileURLToPath(new URL('./studio.html', import.meta.url)),
      },
    },
  },
});
