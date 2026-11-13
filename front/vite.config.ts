/**
 * 前端开发与构建配置；端口固定，避免启动失败时悄悄切到另一个地址。
 */
import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

// Tauri CLI 构建桌面端时设置的平台与调试标记（浏览器开发时不存在）。
const tauriPlatform = process.env.TAURI_ENV_PLATFORM;
const tauriDebug = !!process.env.TAURI_ENV_DEBUG;

export default defineConfig(({ mode }) => {
  // 官网与工作台共享源码，但各自独立构建；Pages 不会拿到工作台及其演示服务代码。
  const studio = mode === 'studio';
  return {
    // 官网通过 VITE_BASE 支持 Pages 子路径；桌面资源使用相对地址。
    base: studio ? './' : (process.env.VITE_BASE ?? '/'),
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
      // Rust 工程的编译产物很多，不需要 Vite 监视。
      watch: { ignored: ['**/src-tauri/**'] },
    },
    preview: { port: 4173, strictPort: true },
    build: {
      outDir: studio ? 'dist/studio' : 'dist/website',
      emptyOutDir: true,
      // Windows 桌面端使用 WebView2（Chromium 内核）；浏览器构建保持 Vite 默认目标。
      ...(tauriPlatform === 'windows' ? { target: 'chrome105' } : {}),
      // 桌面端调试构建保留可读代码与 sourcemap。
      minify: !tauriDebug,
      sourcemap: tauriDebug,
      // 两种产物只包含自己的 HTML 入口，避免官网和工作台互相携带业务代码。
      rollupOptions: {
        input: studio
          ? fileURLToPath(new URL('./studio.html', import.meta.url))
          : fileURLToPath(new URL('./index.html', import.meta.url)),
      },
    },
  };
});
