/**
 * 前端开发与构建配置；端口固定，避免启动失败时悄悄切到另一个地址。
 */
import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  // 将 .vue 单文件组件编译成浏览器可以加载的模块。
  plugins: [vue()],
  // strictPort 阻止端口被占用时自动换号，便于与浏览器测试和接口代理保持一致。
  server: { port: 5173, strictPort: true },
  preview: { port: 4173, strictPort: true },
});
