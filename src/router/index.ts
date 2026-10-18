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
    {
      path: '/tasks',
      name: 'tasks',
      component: () => import('../views/tasks/TasksView.vue'),
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
