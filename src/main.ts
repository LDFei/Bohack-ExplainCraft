import { createApp } from 'vue'
import { createRouter, createWebHistory } from 'vue-router'
import App from './App.vue'
import { api } from './services/api.ts'
import './style.css'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/', component: { template: '<span />' } },
    { path: '/create', component: { template: '<span />' } },
    { path: '/projects/:projectId', component: { template: '<span />' } },
    { path: '/:pathMatch(.*)*', redirect: '/create' },
  ],
})

router.beforeEach(async to => {
  if (to.path !== '/') return
  try {
    const projects = await api.listProjects()
    return projects[0] ? `/projects/${projects[0].id}` : '/create'
  } catch { return '/create' }
})

createApp(App).use(router).mount('#app')
