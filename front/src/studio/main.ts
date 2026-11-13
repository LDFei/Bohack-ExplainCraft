/**
 * 工作区入口：只装载三栏工作台，封面首页由 index.html 与 cover 入口单独打包。
 */
import { createApp } from 'vue';
import { createRouter, createWebHashHistory } from 'vue-router';
import App from '../App.vue';
import { api } from '../services/api.ts';
import '../style.css';
import '../themes.css';
import '../workspace-layout.css';
import { initializeTheme } from '../composables/useTheme.ts';

// 外观先于项目与路由加载，读取偏好不触碰业务存储。
initializeTheme();

// 工作区沿用原路由表，但改用 hash 模式：静态托管（GitHub Pages 等）刷新深链时无需服务端重写。
const router = createRouter({
  history: createWebHashHistory(),
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
  if (to.path !== '/' && to.path !== '/workspace') return;
  try {
    const projects = await api.listProjects();
    return projects[0] ? `/projects/${projects[0].id}` : '/create';
  } catch {
    return '/create';
  }
});

// 注册路由之后挂载页面；全局样式已在入口导入，所有组件共用工作台视觉规则。
createApp(App).use(router).mount('#app');
