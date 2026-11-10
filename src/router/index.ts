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
    // 独立设置窗口：不挂 WorkspaceShell，只渲染设置页（design §3）。
    // 与 /settings 共用同一个 SFC，由路由 name 区分「关闭窗口」与「返回上一页」。
    {
      path: '/settings-window',
      name: 'settings-window',
      component: () => import('../views/settings/SettingsView.vue').then((m) => m.default),
    },
    // 设置页同样挂在 WorkspaceShell 下：侧栏常驻，设置入口在所有页面都可见
    // （浏览器 dev 的回退形态；桌面端入口改为 openSettingsWindow()）
    {
      path: '/settings',
      component: () => import('../layouts/WorkspaceShell.vue'),
      children: [
        {
          path: '',
          name: 'settings',
          component: () => import('../views/settings/SettingsView.vue'),
        },
      ],
    },
    // /tasks = 项目选择页；具体项目的看板走 /tasks/:projectId（'all' = 全部项目）。
    // 二者都挂在 WorkspaceShell 下，保留左侧菜单（与 /automations 一致）。
    {
      path: '/tasks',
      component: () => import('../layouts/WorkspaceShell.vue'),
      children: [
        {
          path: '',
          name: 'tasks',
          component: () => import('../views/tasks/ProjectPickerView.vue'),
        },
        {
          path: ':projectId',
          name: 'task-board',
          component: () => import('../views/tasks/TasksView.vue'),
          props: true,
        },
      ],
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
          // 必须显式取 default：直接返回整段模块命名空间会让打包器生成命名空间包装，
          // 该包装的 helper 目前被分到 Monaco chunk，会把 editor.api 拉回入口静态依赖。
          // 同理，HomeView 的入口预热也必须 .then((m) => m.default)。
          component: () => import('../views/coding/CodingWorkspace.vue').then((m) => m.default),
        },
      ],
    },
  ],
})

export default router
