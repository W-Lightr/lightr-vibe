<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  PanelLeftClose,
  Search,
  SquarePen,
  Ellipsis,
  Zap,
  ListTodo,
  GitBranch,
  Infinity as InfinityIcon,
  FolderOpen,
  FolderInput,
  Settings,
  ChevronRight,
} from 'lucide-vue-next'
import { useWorkspaceStore } from '../../../stores/workspace'
import { useSessionStore } from '../../../stores/session'
import { useProjectStore } from '../../../stores/project'
import SessionItem from './SessionItem.vue'
import type { Session } from '../../../types/session'

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
const projectStore = useProjectStore()

// 侧栏内导航选中项（chat 之外的菜单为占位状态）
type NavKey = 'chat' | 'automation' | 'todos' | 'repo' | 'canvas'
const activeNav = ref<NavKey>('chat')

const navItems: { key: NavKey; label: string; icon: typeof Zap; implemented: boolean }[] = [
  { key: 'automation', label: '自动化', icon: Zap, implemented: false },
  { key: 'todos', label: '待办任务', icon: ListTodo, implemented: false },
  { key: 'repo', label: '仓库面板', icon: GitBranch, implemented: false },
  { key: 'canvas', label: '无限画布', icon: InfinityIcon, implemented: false },
]

// 搜索（开关状态提升到 workspace store，侧栏隐藏时顶栏搜索按钮可联动打开）
const searchOpen = computed({
  get: () => workspace.sidebarSearchOpen,
  set: (v) => (workspace.sidebarSearchOpen = v),
})
const query = ref('')

// 重命名
const renamingId = ref<string | null>(null)
const renameDraft = ref('')
function startRename(id: string, title: string) {
  renamingId.value = id
  renameDraft.value = title
}
function commitRename() {
  if (renamingId.value) sessionStore.renameSession(renamingId.value, renameDraft.value)
  renamingId.value = null
}

function createSession() {
  const projectId = projectStore.folderProjects()[0]?.id
  const session = sessionStore.createSession(workspace.selectedModel, {
    kind: 'folder',
    projectId,
  })
  workspace.openSession(session.id)
  activeNav.value = 'chat'
}

function createTmpSession() {
  const session = sessionStore.createSession(workspace.selectedModel, { kind: 'tmp' })
  workspace.openSession(session.id)
  activeNav.value = 'chat'
}

function removeSession(id: string) {
  sessionStore.deleteSession(id)
  workspace.forgetSession(id)
}

/** 搜索过滤后的会话 */
function filterSessions(list: Session[]) {
  const q = query.value.trim().toLowerCase()
  return list
    .filter((s) => !q || s.title.toLowerCase().includes(q))
    .sort((a, b) => b.updatedAt - a.updatedAt)
}

/** 文件夹分组：每个文件夹项目 + 其下会话 */
const folderGroups = computed(() =>
  projectStore
    .folderProjects()
    .map((project) => ({
      project,
      sessions: filterSessions(sessionStore.sessions.filter((s) => s.projectId === project.id)),
    }))
    .filter((g) => g.sessions.length > 0 || !query.value.trim()),
)

/** 临时聊天会话 */
const tmpSessions = computed(() =>
  filterSessions(sessionStore.sessions.filter((s) => s.kind === 'tmp')),
)

/** 最近会话（全部，按更新时间倒序） */
const recentSessions = computed(() => filterSessions(sessionStore.sessions).slice(0, 8))
const recentOpen = ref(false)
</script>

<template>
  <aside class="flex h-full w-full flex-col border-r border-neutral-200 bg-neutral-50">
    <!-- 顶部工具栏（与标签条等高，顶到窗口顶部） -->
    <div data-drag-region class="flex h-10 shrink-0 items-center gap-0.5 px-2">
      <button
        class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-200/70"
        title="收起侧栏"
        data-no-drag
        @click="workspace.sidebarVisible = false"
      >
        <PanelLeftClose class="h-4 w-4" />
      </button>
      <button
        class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-md hover:bg-neutral-200/70"
        :class="searchOpen ? 'text-neutral-800' : 'text-neutral-500'"
        title="搜索会话"
        data-no-drag
        @click="searchOpen = !searchOpen"
      >
        <Search class="h-4 w-4" />
      </button>
      <button
        class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-200/70"
        title="新建会话"
        data-no-drag
        @click="createSession"
      >
        <SquarePen class="h-4 w-4" />
      </button>
      <div class="flex-1" />
      <button
        class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-200/70"
        title="更多全局功能"
        data-no-drag
      >
        <Ellipsis class="h-4 w-4" />
      </button>
    </div>

    <!-- 搜索框 -->
    <div v-if="searchOpen" class="px-2 pb-1.5">
      <input
        v-model="query"
        placeholder="搜索会话…"
        class="h-7 w-full rounded-md border border-neutral-200 bg-white px-2 text-xs text-neutral-700 outline-none focus:border-neutral-400"
      />
    </div>

    <!-- 功能导航 -->
    <nav class="shrink-0 space-y-0.5 px-2 pb-2">
      <button
        class="flex h-9 w-full cursor-pointer items-center gap-2.5 rounded-md px-2.5 text-[13px] text-neutral-700 hover:bg-neutral-200/60"
        @click="createSession"
      >
        <SquarePen class="h-4 w-4 text-neutral-500" />
        新建会话
      </button>
      <button
        v-for="item in navItems"
        :key="item.key"
        class="flex h-9 w-full cursor-pointer items-center gap-2.5 rounded-md px-2.5 text-[13px]"
        :class="activeNav === item.key ? 'bg-neutral-200/70 text-neutral-800' : 'text-neutral-700 hover:bg-neutral-200/60'"
        @click="activeNav = item.key"
      >
        <component :is="item.icon" class="h-4 w-4 text-neutral-500" />
        {{ item.label }}
        <span v-if="!item.implemented" class="ml-auto rounded bg-neutral-200 px-1 text-[10px] text-neutral-400">
          待接入
        </span>
      </button>
    </nav>

    <!-- 未接入功能的占位状态 -->
    <div
      v-if="activeNav !== 'chat'"
      class="flex flex-1 flex-col items-center justify-center gap-1 px-6 text-center"
    >
      <p class="text-xs text-neutral-500">「{{ navItems.find((n) => n.key === activeNav)?.label }}」尚未接入</p>
      <button class="mt-1 cursor-pointer text-xs text-blue-500 hover:underline" @click="activeNav = 'chat'">
        返回会话列表
      </button>
    </div>

    <!-- 会话区：文件夹 / 聊天（临时会话）/ 最近 -->
    <div v-else class="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
      <!-- 文件夹 -->
      <div class="mb-1 flex items-center justify-between px-2.5 pt-1 pb-1">
        <span class="text-[11px] text-neutral-400">文件夹</span>
        <button
          class="cursor-pointer rounded p-0.5 text-neutral-400 hover:bg-neutral-200 hover:text-neutral-600"
          title="打开文件夹"
        >
          <FolderInput class="h-3.5 w-3.5" />
        </button>
      </div>
      <div v-for="group in folderGroups" :key="group.project.id" class="mb-2">
        <div class="flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs text-neutral-600 hover:bg-neutral-200/50">
          <FolderOpen class="h-4 w-4 text-neutral-500" />
          <span class="flex-1 truncate">{{ group.project.name }}</span>
          <span class="text-[10px] text-neutral-400">{{ group.sessions.length }}</span>
        </div>
        <div class="ml-4 border-l border-neutral-300/80 pl-1.5">
          <SessionItem
            v-for="s in group.sessions"
            :key="s.id"
            :session="s"
            :active="s.id === workspace.activeSessionId"
            :renaming="renamingId === s.id"
            v-model:rename-draft="renameDraft"
            @select="workspace.openSession(s.id)"
            @rename="startRename(s.id, s.title)"
            @commit-rename="commitRename"
            @cancel-rename="renamingId = null"
            @remove="removeSession(s.id)"
          />
          <p v-if="group.sessions.length === 0" class="px-2.5 py-1.5 text-[11px] text-neutral-400">
            暂无会话
          </p>
        </div>
      </div>

      <!-- 聊天（临时会话） -->
      <div class="mb-1 flex items-center justify-between px-2.5 pt-1 pb-1">
        <span class="text-[11px] text-neutral-400">聊天</span>
        <button
          class="cursor-pointer rounded p-0.5 text-neutral-400 hover:bg-neutral-200 hover:text-neutral-600"
          title="新建临时会话"
          @click="createTmpSession"
        >
          <SquarePen class="h-3.5 w-3.5" />
        </button>
      </div>
      <SessionItem
        v-for="s in tmpSessions"
        :key="s.id"
        :session="s"
        :active="s.id === workspace.activeSessionId"
        :renaming="renamingId === s.id"
        v-model:rename-draft="renameDraft"
        @select="workspace.openSession(s.id)"
        @rename="startRename(s.id, s.title)"
        @commit-rename="commitRename"
        @cancel-rename="renamingId = null"
        @remove="removeSession(s.id)"
      />
      <p v-if="tmpSessions.length === 0" class="px-2.5 py-1.5 text-[11px] text-neutral-400">
        暂无临时会话
      </p>

      <!-- 最近 -->
      <button
        class="mt-1 flex w-full cursor-pointer items-center gap-1 px-2.5 py-1.5 text-[11px] text-neutral-400 hover:text-neutral-600"
        @click="recentOpen = !recentOpen"
      >
        最近
        <ChevronRight class="h-3 w-3 transition-transform" :class="recentOpen && 'rotate-90'" />
      </button>
      <div v-if="recentOpen" class="ml-2 border-l border-neutral-300/80 pl-1.5">
        <SessionItem
          v-for="s in recentSessions"
          :key="`recent-${s.id}`"
          :session="s"
          :active="s.id === workspace.activeSessionId"
          :renaming="false"
          :rename-draft="''"
          @select="workspace.openSession(s.id)"
          @rename="startRename(s.id, s.title)"
          @commit-rename="commitRename"
          @cancel-rename="renamingId = null"
          @remove="removeSession(s.id)"
        />
      </div>

      <p
        v-if="query && folderGroups.every((g) => g.sessions.length === 0) && tmpSessions.length === 0"
        class="px-2.5 py-3 text-[11px] text-neutral-400"
      >
        没有匹配的会话
      </p>
    </div>

    <!-- 底部状态栏 -->
    <div class="flex h-9 shrink-0 items-center justify-between border-t border-neutral-200 px-2">
      <button
        class="flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-xs text-neutral-600 hover:bg-neutral-200/60"
      >
        <Settings class="h-3.5 w-3.5" />
        设置
      </button>
      <span class="pr-1 text-[11px] text-neutral-400">{{ sessionStore.sessions.length }} 个会话</span>
    </div>
  </aside>
</template>
