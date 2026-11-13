/**
 * Vue 启动入口：建立页面路由、处理首页落点、载入全局样式并挂载 App。
 */
import { createApp } from 'vue';
import { createRouter, createWebHistory } from 'vue-router';
import App from './App.vue';
import { api } from './services/api.ts';
import './style.css';

// 根 App 根据路由读取同一套工作台；占位组件只登记路径，当前没有单独渲染 RouterView。
const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', component: { template: '<span />' } },
    { path: '/create', component: { template: '<span />' } },
    { path: '/projects/:projectId', component: { template: '<span />' } },
    { path: '/:pathMatch(.*)*', redirect: '/create' },
  ],
});

// 首页优先打开最近的项目；列表为空或读取失败时回到新建页，避免固定跳到已删除的示例。
router.beforeEach(async (to) => {
  if (to.path !== '/') return;
  try {
    const projects = await api.listProjects();
    return projects[0] ? `/projects/${projects[0].id}` : '/create';
  } catch {
    return '/create';
  }
});

// 注册路由之后挂载页面；全局样式已在入口导入，所有组件共用工作台视觉规则。
createApp(App).use(router).mount('#app');
