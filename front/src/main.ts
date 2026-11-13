/**
 * Vue 启动入口：建立页面路由、处理首页落点、载入全局样式并挂载 App。
 */
import { createApp } from 'vue';
import { createRouter, createWebHistory } from 'vue-router';
import App from './App.vue';
import { api } from './services/api.ts';
import './style.css';
import './aceternity.css';
import './workspace-layout.css';
import { initializeTheme } from './composables/useTheme.ts';

// 外观先于项目与路由加载，读取偏好不触碰业务存储。
initializeTheme();

// 首页展示产品；/workspace 延续原首页的最近项目入口，已有创作与项目路由保持有效。
const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', component: { template: '<span />' } },
    { path: '/workspace', component: { template: '<span />' } },
    { path: '/create', component: { template: '<span />' } },
    { path: '/projects/:projectId', component: { template: '<span />' } },
    { path: '/:pathMatch(.*)*', redirect: '/create' },
  ],
});

// 工作台入口优先打开最近的项目；列表为空或读取失败时回到新建页，不固定打开示例。
router.beforeEach(async (to) => {
  if (to.path !== '/workspace') return;
  try {
    const projects = await api.listProjects();
    return projects[0] ? `/projects/${projects[0].id}` : '/create';
  } catch {
    return '/create';
  }
});

// 注册路由之后挂载页面；全局样式已在入口导入，所有组件共用工作台视觉规则。
createApp(App).use(router).mount('#app');
