import { createRouter, createWebHistory } from 'vue-router'
import HomeView from '../views/HomeView.vue'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      name: 'home',
      component: HomeView,
    },
    {
      path: '/settings',
      name: 'settings',
      component: () => import('../views/settings/SettingsView.vue'),
    },
    // /tasks = 项目选择页；具体项目的看板走 /tasks/:projectId（'all' = 全部项目）
    {
      path: '/tasks',
      name: 'tasks',
      component: () => import('../views/tasks/ProjectPickerView.vue'),
    },
    {
      path: '/tasks/:projectId',
      name: 'task-board',
      component: () => import('../views/tasks/TasksView.vue'),
      props: true,
    },
    // 自动化页挂在 WorkspaceShell 下：保留左侧菜单（设计图要求）
    {
      path: '/automations',
      component: () => import('../layouts/WorkspaceShell.vue'),
      children: [
        {
          path: '',
          name: 'automations',
          component: () => import('../views/automations/AutomationsView.vue'),
        },
      ],
    },
    {
      path: '/coding',
      component: () => import('../layouts/WorkspaceLayout.vue'),
      children: [
        {
          path: '',
          name: 'coding',
          component: () => import('../views/coding/CodingWorkspace.vue'),
        },
      ],
    },
  ],
})

export default router
