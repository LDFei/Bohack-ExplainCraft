/**
 * 封面入口：只装载产品首页，不引入工作台状态、接口层和编辑器组件。
 */
import { createApp } from 'vue';
import CoverApp from './CoverApp.vue';
import '../style.css';
import '../aceternity.css';
import '../themes.css';
import { initializeTheme } from '../composables/useTheme.ts';

// 主题偏好与工作区共用同一份存储，封面切换主题后进入工作台仍然生效。
initializeTheme();

createApp(CoverApp).mount('#app');
