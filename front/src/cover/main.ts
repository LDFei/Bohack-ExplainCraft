/**
 * 封面入口：只装载产品首页，不引入工作台状态、接口层和编辑器组件。
 */
import { createApp } from 'vue';
import CoverApp from './CoverApp.vue';
import '../style.css';
import '../aceternity.css';
import '../themes.css';
import { initializeTheme } from '../composables/useTheme.ts';

// 官网和桌面端使用同一套主题变量；各自只保存当前设备的显示偏好。
initializeTheme();

createApp(CoverApp).mount('#app');
