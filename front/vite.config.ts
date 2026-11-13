/**
 * 前端开发与构建配置；端口固定，避免启动失败时悄悄切到另一个地址。
 */
import { fileURLToPath, URL } from 'node:url';
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  // GitHub Pages 之类的子路径部署通过 VITE_BASE 传入仓库前缀，本地构建保持根路径。
  base: process.env.VITE_BASE ?? '/',
  // 将 .vue 单文件组件编译成浏览器可以加载的模块。
  plugins: [vue()],
  // strictPort 阻止端口被占用时自动换号，便于与浏览器测试和接口代理保持一致。
  server: { port: 5173, strictPort: true },
  preview: { port: 4173, strictPort: true },
  build: {
    // 封面与工作区各自产出 HTML：index.html 只含首页，studio.html 才是工作台。
    rollupOptions: {
      input: {
        cover: fileURLToPath(new URL('./index.html', import.meta.url)),
        studio: fileURLToPath(new URL('./studio.html', import.meta.url)),
      },
    },
  },
});
