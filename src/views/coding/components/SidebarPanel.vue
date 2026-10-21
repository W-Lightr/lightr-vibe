<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { NPopconfirm, useMessage } from 'naive-ui'
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
  ChevronRight,
  Trash2,
} from 'lucide-vue-next'
import { useWorkspaceStore } from '../../../stores/workspace'
import { useSessionStore } from '../../../stores/session'
import { useProjectStore } from '../../../stores/project'
import { useConversation, ensureModelOptions } from '../../../composables/useConversation'
import { canPickFolder, fsService, projectNameFromPath } from '../../../services/fsService'
import { formatRelativeTime } from '../../../utils/time'
import SessionItem from './SessionItem.vue'
import NewSessionDialog from './NewSessionDialog.vue'
import type { Session, NewSessionTarget } from '../../../types/session'
import type { EngineWire } from '../../../types/engine'

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
const projectStore = useProjectStore()
const { closeConnection } = useConversation()
const message = useMessage()
const router = useRouter()

/** 原生目录对话框仅桌面端可用；浏览器 dev 环境按钮禁用态（R1） */
const pickAvailable = canPickFolder()
const picking = ref(false)

/** 「打开文件夹」：原生对话框选目录 → 校验可读 → 加入项目列表并持久化 */
async function openFolder() {
  if (!pickAvailable || picking.value) return
  picking.value = true
  try {
    const picked = await fsService.pickFolder()
    if (!picked) return // 用户取消
    // 校验目录存在且可读：列一次根目录，失败则拒绝加入
    try {
      await fsService.listDir(picked, picked)
    } catch {
      message.error('无法读取该目录，请确认目录存在且有访问权限')
      return
    }
    const project = projectStore.addProject(picked, projectNameFromPath(picked))
    message.success(`已打开文件夹「${project.name}」`)
  } catch {
    message.error('打开文件夹失败')
  } finally {
    picking.value = false
  }
}

/** 移除项目：仅删索引不删磁盘文件；其会话收敛为孤儿（projectId 置空归入「聊天」组，
 * store action 统一处理 + 索引落盘，design D4 / 子任务 1 遗留项 3） */
function removeProject(projectId: string) {
  sessionStore.orphanSessionsOf(projectId)
  projectStore.removeProject(projectId)
}

// 侧栏内导航选中项（chat 之外的菜单为占位状态）
type NavKey = 'chat' | 'automation' | 'todos' | 'repo' | 'canvas'
const activeNav = ref<NavKey>('chat')

/** 已接入的导航项走 `to` 路由跳转；其余为占位（侧栏内显示「尚未接入」） */
const navItems: { key: NavKey; label: string; icon: typeof Zap; to?: string }[] = [
  { key: 'automation', label: '自动化', icon: Zap },
  { key: 'todos', label: '待办任务', icon: ListTodo, to: '/tasks' },
  { key: 'repo', label: '仓库面板', icon: GitBranch },
  { key: 'canvas', label: '无限画布', icon: InfinityIcon },
]

/** 点击导航项：已接入的直接跳转（侧栏在 /coding 下，任务看板在 /tasks） */
function onNavClick(item: { key: NavKey; to?: string }) {
  if (item.to) {
    router.push(item.to)
    return
  }
  activeNav.value = item.key
}

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
  // 通用新建：弹窗选引擎 + 文件夹（PRD R2）；确认后创建草稿
  pendingProjectId.value = null
  dialogOpen.value = true
}

function createTmpSession() {
  // 临时会话：同样走弹窗（可选引擎 / 文件夹，默认最近项目）
  pendingProjectId.value = null
  dialogOpen.value = true
}

/** 按目标创建草稿会话并打开（延迟入列，PRD D3：不立即入列表/落盘）；返回新会话 id */
function createDraftFor(target: NewSessionTarget, engineId: EngineWire): string {
  if (target.kind === 'tmp') {
    const session = sessionStore.createDraft(workspace.selectedModel, { kind: 'tmp', engineId })
    workspace.openSession(session.id)
    return session.id
  }
  const project = projectStore.get(target.projectId)
  const session = sessionStore.createDraft(workspace.selectedModel, {
    kind: 'folder',
    projectId: target.projectId,
    cwd: project?.rootPath,
    engineId,
  })
  workspace.openSession(session.id)
  return session.id
}

/** 某文件夹行内的「新建会话」：弹窗选引擎（文件夹已确定），确认后按该文件夹建草稿（PRD R1/D1） */
function createSessionInProject(projectId: string) {
  pendingProjectId.value = projectId
  dialogOpen.value = true
}

/** 新建会话弹窗：确认后按所选引擎 + 目标创建草稿（engineId 写入 Session，PRD R2） */
const dialogOpen = ref(false)
/** engineOnly 模式下待绑定的文件夹（由文件夹行触发） */
const pendingProjectId = ref<string | null>(null)

async function onDialogConfirm(payload: { engineId: EngineWire; target: NewSessionTarget }) {
  const sessionId = createDraftFor(payload.target, payload.engineId)
  activeNav.value = 'chat'
  // 选择引擎后立即预取该引擎的模型列表（probe），使 composer 模型下拉打开即有值；
  // 失败静默（composer 仍会异步兜底，不阻塞）
  if (sessionId) await ensureModelOptions(sessionId).catch(() => undefined)
}

function removeSession(id: string) {
  // 删除会话前销毁其 ACP 连接（生成中先 cancel，再 session/close + 杀进程）
  void closeConnection(id)
  sessionStore.deleteSession(id)
  workspace.forgetSession(id)
}

// ---- 文件夹折叠（PRD 10-21-sidebar-folder-collapse：R1–R4） ----

const COLLAPSED_KEY = 'lightr.sidebar.collapsedProjects'

/** 同步读取折叠记录；脏数据 / 隐私模式读不到时按「全部展开」处理（R3） */
function readCollapsed(): Set<string> {
  try {
    const raw = window.localStorage.getItem(COLLAPSED_KEY)
    if (!raw) return new Set()
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return new Set()
    return new Set(parsed.filter((id): id is string => typeof id === 'string'))
  } catch {
    return new Set()
  }
}

/** 已收起的项目 id；整体替换而非原地改 Set，保证模板里的读取最直观 */
const collapsedProjects = ref<Set<string>>(readCollapsed())

function writeCollapsed(set: Set<string>) {
  try {
    window.localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...set]))
  } catch {
    // 记忆失败不影响当前会话
  }
}

/** 仅箭头调用：切换折叠（整行点击仍走 openProject，R1/AC3） */
function toggleCollapse(projectId: string) {
  const next = new Set(collapsedProjects.value)
  if (next.has(projectId)) next.delete(projectId)
  else next.add(projectId)
  collapsedProjects.value = next
  writeCollapsed(next)
}

/** 搜索中忽略折叠，命中会话一律显示（R4/AC6） */
const searching = computed(() => query.value.trim().length > 0)

function isCollapsed(projectId: string): boolean {
  return !searching.value && collapsedProjects.value.has(projectId)
}

/** 某项目下是否存在生成中的会话（收起时用于计数旁蓝点，R2/AC5） */
function hasGenerating(list: Session[]): boolean {
  return list.some((s) => workspace.isGenerating(s.id))
}

/** 搜索过滤后的会话 */
function filterSessions(list: Session[]) {
  const q = query.value.trim().toLowerCase()
  return list
    .filter((s) => !q || s.title.toLowerCase().includes(q))
    .sort((a, b) => b.updatedAt - a.updatedAt)
}

/** 文件夹分组：每个文件夹项目 + 其下会话；计数用搜索前的总数，避免折叠/搜索时数字跳动 */
const folderGroups = computed(() =>
  projectStore.projects
    .map((project) => {
      const all = sessionStore.sessions.filter((s) => s.projectId === project.id)
      const sessions = filterSessions(all)
      return {
        project,
        sessions,
        total: all.length,
        generating: hasGenerating(all),
        collapsed: isCollapsed(project.id),
      }
    })
    .filter((g) => g.sessions.length > 0 || !query.value.trim()),
)

/** 临时聊天会话（含项目被移除后归入的孤儿会话，design D4） */
const tmpSessions = computed(() =>
  filterSessions(
    sessionStore.sessions.filter((s) => s.kind === 'tmp' || !projectStore.get(s.projectId)),
  ),
)

/** 「最近」分组：lastOpenedAt 倒序前 5 个项目（R3） */
const recentProjects = computed(() => projectStore.recentProjects)
const recentOpen = ref(false)

/** 点击文件夹行：仅激活项目（刷新 lastOpenedAt 供「最近」排序），
 * 不自动打开/新建会话——打开具体会话需点击会话项本身（用户诉求 2026-10）。 */
function openProject(projectId: string) {
  projectStore.touchProject(projectId)
  activeNav.value = 'chat'
}
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
        @click="onNavClick(item)"
      >
        <component :is="item.icon" class="h-4 w-4 text-neutral-500" />
        {{ item.label }}
        <span v-if="!item.to" class="ml-auto rounded bg-neutral-200 px-1 text-[10px] text-neutral-400">
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
        <!-- 打开文件夹：桌面端弹原生目录对话框；浏览器 dev 禁用态（R1） -->
        <button
          class="cursor-pointer rounded p-0.5 text-neutral-400 hover:bg-neutral-200 hover:text-neutral-600 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent"
          :title="pickAvailable ? '打开文件夹' : '打开文件夹（桌面端可用）'"
          :disabled="!pickAvailable || picking"
          data-no-drag
          @click="openFolder"
        >
          <FolderInput class="h-3.5 w-3.5" />
        </button>
      </div>
      <div v-for="group in folderGroups" :key="group.project.id" class="group/proj mb-2">
        <div
          class="flex h-7 cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-xs text-neutral-600 hover:bg-neutral-200/50"
          :title="group.project.rootPath"
          @click="openProject(group.project.id)"
        >
          <!-- 折叠箭头：仅此区域切换折叠，整行点击仍是「打开该项目」 -->
          <button
            class="-ml-1 flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-300/60 hover:text-neutral-700"
            :title="group.collapsed ? '展开文件夹' : '收起文件夹'"
            @click.stop="toggleCollapse(group.project.id)"
          >
            <ChevronRight
              class="h-3.5 w-3.5 transition-transform"
              :class="!group.collapsed && 'rotate-90'"
            />
          </button>
          <FolderOpen class="h-4 w-4 shrink-0 text-neutral-500" />
          <span class="min-w-0 flex-1 truncate">{{ group.project.name }}</span>
          <!-- 搜索时显示匹配数（R4）；其余时候显示总数 -->
          <span
            v-if="searching"
            class="shrink-0 text-[10px] text-neutral-400"
          >{{ group.sessions.length }} 个匹配</span>
          <span
            v-else
            class="flex shrink-0 items-center gap-1 text-[10px] text-neutral-400"
            :class="group.generating && group.collapsed ? '' : 'group-hover/proj:hidden'"
          >
            {{ group.total }}
            <!-- 收起且有生成中会话：蓝点提示（R2/AC5） -->
            <span
              v-if="group.generating && group.collapsed"
              class="h-1.5 w-1.5 rounded-full bg-blue-500 animate-pulse"
            />
          </span>
          <!-- 悬浮操作：在该文件夹下新建会话（不弹窗，直接绑定本项目建草稿，PRD R1） -->
          <button
            class="hidden h-4.5 w-4.5 shrink-0 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-300/60 hover:text-neutral-700 group-hover/proj:flex"
            title="在此文件夹新建会话"
            @click.stop="createSessionInProject(group.project.id)"
          >
            <SquarePen class="h-3 w-3" />
          </button>
          <!-- 悬浮操作：移除项目（仅删索引，不删磁盘文件） -->
          <NPopconfirm @positive-click="removeProject(group.project.id)">
            <template #trigger>
              <button
                class="hidden h-4.5 w-4.5 shrink-0 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-300/60 hover:text-red-500 group-hover/proj:flex"
                title="移除项目（不删除磁盘文件）"
                @click.stop
              >
                <Trash2 class="h-3 w-3" />
              </button>
            </template>
            移除项目「{{ group.project.name }}」？磁盘文件不会被删除，其会话将归入「聊天」组。
          </NPopconfirm>
        </div>
        <!-- 收起时不渲染会话列表；仍然保留空项目提示只在展开态出现 -->
        <div v-if="!group.collapsed" class="ml-4 border-l border-neutral-300/80 pl-1.5">
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
      <p v-if="folderGroups.length === 0" class="px-2.5 py-1 text-[11px] text-neutral-400">
        {{ pickAvailable ? '暂无项目，点击右上按钮打开文件夹' : '暂无项目（浏览器模式为 mock 数据）' }}
      </p>
      <!-- 浏览器 dev mock 提示条（AC7，样式沿用设置页 mock 提示模式） -->
      <p v-if="!pickAvailable" class="px-2.5 pb-1 text-[10px] text-neutral-400">
        浏览器模式：项目与文件为 mock 数据，「打开文件夹」桌面端可用
      </p>

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

      <!-- 最近：lastOpenedAt 倒序前 5 个项目（父任务 Q3） -->
      <button
        class="mt-1 flex w-full cursor-pointer items-center gap-1 px-2.5 py-1.5 text-[11px] text-neutral-400 hover:text-neutral-600"
        @click="recentOpen = !recentOpen"
      >
        最近
        <ChevronRight class="h-3 w-3 transition-transform" :class="recentOpen && 'rotate-90'" />
      </button>
      <div v-if="recentOpen" class="ml-2 border-l border-neutral-300/80 pl-1.5">
        <div
          v-for="p in recentProjects"
          :key="`recent-${p.id}`"
          class="group/recent flex h-9 cursor-pointer items-center gap-2 rounded-md px-2.5 text-[13px] text-neutral-600 hover:bg-neutral-200/50"
          :title="p.rootPath"
          @click="openProject(p.id)"
        >
          <FolderOpen class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
          <span class="min-w-0 flex-1 truncate">{{ p.name }}</span>
          <span class="shrink-0 text-[10px] text-neutral-400 group-hover/recent:hidden">
            {{ formatRelativeTime(p.lastOpenedAt) }}
          </span>
        </div>
        <p v-if="recentProjects.length === 0" class="px-2.5 py-1.5 text-[11px] text-neutral-400">
          暂无最近项目
        </p>
      </div>

      <p
        v-if="query && folderGroups.every((g) => g.sessions.length === 0) && tmpSessions.length === 0"
        class="px-2.5 py-3 text-[11px] text-neutral-400"
      >
        没有匹配的会话
      </p>
    </div>

    <!-- 底部状态栏 -->
    <div class="flex h-9 shrink-0 items-center justify-end border-t border-neutral-200 px-3">
      <span class="pr-1 text-[11px] text-neutral-400">{{ sessionStore.sessions.length }} 个会话</span>
    </div>

    <!-- 新建会话弹窗（选引擎 + 目标，PRD R5）；文件夹行触发时为 engineOnly（文件夹已定） -->
    <NewSessionDialog
      :open="dialogOpen"
      :engine-only="pendingProjectId !== null"
      :default-project-id="pendingProjectId"
      @close="(dialogOpen = false), (pendingProjectId = null)"
      @confirm="onDialogConfirm"
    />
  </aside>
</template>
