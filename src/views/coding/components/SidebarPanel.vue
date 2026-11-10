<script setup lang="ts">
import { computed, h, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NDropdown, NPopconfirm, useDialog, useMessage } from 'naive-ui'
import type { DropdownDividerOption, DropdownOption } from 'naive-ui'
import {
  PanelLeftClose,
  Search,
  Settings,
  SquarePen,
  Ellipsis,
  Zap,
  ListTodo,
  GitBranch,
  Infinity as InfinityIcon,
  FolderOpen,
  FolderInput,
  ChevronRight,
  ChevronDown,
  Trash2,
} from 'lucide-vue-next'
import { useWorkspaceStore } from '../../../stores/workspace'
import { useSessionStore } from '../../../stores/session'
import { useProjectStore } from '../../../stores/project'
import { useConversation, ensureModelOptions } from '../../../composables/useConversation'
import { canPickFolder, fsService, projectNameFromPath } from '../../../services/fsService'
import { formatRelativeTime } from '../../../utils/time'
import { openSettingsWindow } from '../../../utils/settingsWindow'
import SessionItem from './SessionItem.vue'
import NewSessionDialog from './NewSessionDialog.vue'
import type { Session, NewSessionTarget } from '../../../types/session'
import type { EngineWire } from '../../../types/engine'

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
const projectStore = useProjectStore()
const { closeConnection } = useConversation()
const message = useMessage()
const dialog = useDialog()
const router = useRouter()
const route = useRoute()

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

// ---- 文件夹右键菜单：新建会话 / 打开文件资源 / 删除 ----

const folderMenu = ref<{
  open: boolean
  x: number
  y: number
  projectId: string | null
}>({ open: false, x: 0, y: 0, projectId: null })

/** 菜单项；「打开文件资源」仅桌面端可用（浏览器 dev 无文件管理器） */
const folderMenuOptions: (DropdownOption | DropdownDividerOption)[] = [
  {
    key: 'new-session',
    label: '新建会话',
    icon: () => h(SquarePen, { class: 'h-3.5 w-3.5' }),
  },
  {
    key: 'open-folder',
    label: '打开文件资源',
    disabled: !pickAvailable,
    icon: () => h(FolderOpen, { class: 'h-3.5 w-3.5' }),
  },
  { key: 'divider', type: 'divider' },
  {
    key: 'remove',
    label: '删除',
    icon: () => h(Trash2, { class: 'h-3.5 w-3.5' }),
    props: { style: 'color: #dc2626' },
  },
]

/** 在鼠标位置打开文件夹菜单；阻止系统默认菜单 */
function openFolderMenu(e: MouseEvent, projectId: string) {
  folderMenu.value = { open: true, x: e.clientX, y: e.clientY, projectId }
}

function closeFolderMenu() {
  if (folderMenu.value.open) folderMenu.value = { ...folderMenu.value, open: false }
}

/**
 * 用系统文件管理器打开项目根目录（进入目录内部，而不是在父目录中选中）。
 *
 * 走 Rust 自定义命令 open_folder：opener 插件的 open_path 需要静态路径 scope，
 * 无法覆盖用户任意选择的项目目录，因此由 Rust 侧直接调用 opener。
 */
async function openProjectFolder(projectId: string) {
  const project = projectStore.get(projectId)
  if (!project) return
  if (!pickAvailable) {
    message.info('打开文件资源仅桌面端可用')
    return
  }
  try {
    await fsService.openFolder(project.rootPath)
  } catch {
    message.error('打开文件资源失败')
  }
}

/** 右键菜单删除：先弹确认，避免误触 */
function confirmRemoveProject(projectId: string) {
  const project = projectStore.get(projectId)
  if (!project) return
  dialog.warning({
    title: '移除项目',
    content: `移除项目「${project.name}」？磁盘文件不会被删除，其会话将归入「聊天」组。`,
    positiveText: '移除',
    negativeText: '取消',
    onPositiveClick: () => removeProject(projectId),
  })
}

function onFolderMenuSelect(key: string | number) {
  const projectId = folderMenu.value.projectId
  closeFolderMenu()
  if (!projectId) return
  if (key === 'new-session') createSessionInProject(projectId)
  else if (key === 'open-folder') void openProjectFolder(projectId)
  else if (key === 'remove') confirmRemoveProject(projectId)
}

// 侧栏内导航选中项（chat 之外的菜单为占位状态）
type NavKey = 'chat' | 'automation' | 'todos' | 'repo' | 'canvas'
const activeNav = ref<NavKey>('chat')

/**
 * 侧栏高亮跟随路由。
 *
 * 之前只有「点了按钮」才更新 activeNav，所以从别处（或刷新）直接进
 * /automations 时高亮还停在「聊天」，看起来像没切过去。
 */
/** 当前是否在设置页（用于底部设置按钮的高亮） */
const isSettingsRoute = computed(() => route.path.startsWith('/settings'))

/**
 * 打开设置。
 *
 * 桌面端走独立单例设置窗口（openSettingsWindow），主窗口停留原页面；
 * 浏览器 dev 或创建失败时回退页内 /settings（design §7）。
 */
async function openSettings(): Promise<void> {
  try {
    if (await openSettingsWindow()) return
  } catch (e) {
    message.error(`打开设置窗口失败：${e instanceof Error ? e.message : String(e)}`)
    return
  }
  if (!isSettingsRoute.value) void router.push('/settings')
}

function navKeyForPath(path: string): NavKey {
  if (path.startsWith('/automations')) return 'automation'
  if (path.startsWith('/tasks')) return 'todos'
  return 'chat'
}
watch(
  () => route.path,
  (path) => {
    activeNav.value = navKeyForPath(path)
  },
  { immediate: true },
)

/** 已接入的导航项走 `to` 路由跳转；其余为占位（侧栏内显示「尚未接入」） */
const navItems: { key: NavKey; label: string; icon: typeof Zap; to?: string }[] = [
  { key: 'automation', label: '自动化', icon: Zap, to: '/automations' },
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

/**
 * 点会话：先确保在编码工作区，再打开标签。
 *
 * 侧栏被 /automations 复用后，直接 openSession 会让标签打开在一个看不到它的
 * 页面上（用户点了没反应）。
 */
function openSessionFromSidebar(sessionId: string): void {
  ensureCodingRoute()
  workspace.openSession(sessionId)
}

/**
 * 当前不在编码工作区时切回去。
 *
 * 侧栏被 /automations、/tasks 复用后，「新建会话」「点会话」这类动作必须
 * 先把路由切回 /coding —— 否则会话被打开在一个看不到它的页面上。
 */
function ensureCodingRoute(): void {
  if (!route.path.startsWith('/coding')) void router.push('/coding')
}

function createSession() {
  // 侧栏现在也挂在 /automations 上：新建会话必须先回到编码工作区，
  // 否则路由停在自动化页，用户看不到刚打开的会话。
  ensureCodingRoute()
  pendingProjectId.value = null
  dialogOpen.value = true
}

function createTmpSession() {
  ensureCodingRoute()
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

/** 切换折叠。箭头与文件夹行都会调用；行内按钮各自 @click.stop 不会触发。 */
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

// ---- 会话分页：每组默认 5 个，「显示更多」每次追加 5 个 ----

/** 每组首屏最多展示的会话数 */
const SESSION_PAGE_SIZE = 5
/** 临时会话分组的 key（避免与 project id 冲突） */
const TMP_GROUP_KEY = '__tmp__'
/** 各分组已展开到的数量；未记录时按 SESSION_PAGE_SIZE 处理 */
const visibleCounts = ref<Record<string, number>>({})

function visibleCountOf(key: string): number {
  return visibleCounts.value[key] ?? SESSION_PAGE_SIZE
}

function showMoreSessions(key: string): void {
  visibleCounts.value = {
    ...visibleCounts.value,
    [key]: visibleCountOf(key) + SESSION_PAGE_SIZE,
  }
}

/** 文件夹分组：每个文件夹项目 + 其下会话；计数用搜索前的总数，避免折叠/搜索时数字跳动 */
const folderGroups = computed(() =>
  projectStore.projects
    .map((project) => {
      const all = sessionStore.sessions.filter((s) => s.origin !== 'delegation' && s.projectId === project.id)
      const sessions = filterSessions(all)
      const limit = visibleCountOf(project.id)
      return {
        project,
        sessions,
        // 搜索时忽略分页，命中项全部显示（与搜索忽略折叠保持一致）
        visibleSessions: searching.value ? sessions : sessions.slice(0, limit),
        hasMore: !searching.value && sessions.length > limit,
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
    sessionStore.sessions.filter((s) => s.origin !== 'delegation' && (s.kind === 'tmp' || !projectStore.get(s.projectId))),
  ),
)

/** 临时会话的可见切片：搜索时全部显示，否则按分组展开数量截断 */
const visibleTmpSessions = computed(() => {
  const limit = visibleCountOf(TMP_GROUP_KEY)
  return searching.value ? tmpSessions.value : tmpSessions.value.slice(0, limit)
})

const tmpHasMore = computed(
  () => !searching.value && tmpSessions.value.length > visibleCountOf(TMP_GROUP_KEY),
)

/** 「最近」分组：lastOpenedAt 倒序前 5 个项目（R3） */
const recentProjects = computed(() => projectStore.recentProjects)
const recentOpen = ref(false)

/** 点击文件夹行：激活项目（刷新 lastOpenedAt 供「最近」排序）+ 切换折叠。
 *  不自动打开/新建会话——打开具体会话需点击会话项本身（用户诉求 2026-10）。
 *  行内的「新建会话 / 移除项目」按钮各自 @click.stop，不触发这里。 */
function onFolderRowClick(projectId: string) {
  activateProject(projectId)
  toggleCollapse(projectId)
}

/** 仅激活项目、切换折叠（「最近」分组用：那一组没有折叠概念） */
function activateProject(projectId: string) {
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
      v-if="activeNav !== 'chat' && !navItems.find((n) => n.key === activeNav)?.to"
      class="flex flex-1 flex-col items-center justify-center gap-1 px-6 text-center"
    >
      <p class="text-xs text-neutral-500">「{{ navItems.find((n) => n.key === activeNav)?.label }}」尚未接入</p>
      <button class="mt-1 cursor-pointer text-xs text-blue-500 hover:underline" @click="activeNav = 'chat'">
        返回会话列表
      </button>
    </div>

    <!-- 会话区：文件夹 / 聊天（临时会话）/ 最近 -->
    <div v-else class="min-h-0 flex-1 overflow-y-auto px-2 pb-2" @scroll="closeFolderMenu">
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
          @contextmenu.prevent.stop="openFolderMenu($event, group.project.id)"
          @click="onFolderRowClick(group.project.id)"
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
            v-for="s in group.visibleSessions"
            :key="s.id"
            :session="s"
            :active="s.id === workspace.activeSessionId"
            :renaming="renamingId === s.id"
            v-model:rename-draft="renameDraft"
            @select="openSessionFromSidebar(s.id)"
            @rename="startRename(s.id, s.title)"
            @commit-rename="commitRename"
            @cancel-rename="renamingId = null"
            @remove="removeSession(s.id)"
          />
          <p v-if="group.sessions.length === 0" class="px-2.5 py-1.5 text-[11px] text-neutral-400">
            暂无会话
          </p>
          <button
            v-else-if="group.hasMore"
            class="flex w-full cursor-pointer items-center gap-1 rounded-md px-2.5 py-1.5 text-[11px] text-neutral-400 hover:bg-neutral-200/50 hover:text-neutral-600"
            title="每次再显示 5 个会话"
            @click="showMoreSessions(group.project.id)"
          >
            显示更多（还有 {{ group.sessions.length - group.visibleSessions.length }} 个）
            <ChevronDown class="h-3 w-3" />
          </button>
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
        v-for="s in visibleTmpSessions"
        :key="s.id"
        :session="s"
        :active="s.id === workspace.activeSessionId"
        :renaming="renamingId === s.id"
        v-model:rename-draft="renameDraft"
        @select="openSessionFromSidebar(s.id)"
        @rename="startRename(s.id, s.title)"
        @commit-rename="commitRename"
        @cancel-rename="renamingId = null"
        @remove="removeSession(s.id)"
      />
      <p v-if="tmpSessions.length === 0" class="px-2.5 py-1.5 text-[11px] text-neutral-400">
        暂无临时会话
      </p>
      <button
        v-else-if="tmpHasMore"
        class="flex w-full cursor-pointer items-center gap-1 rounded-md px-2.5 py-1.5 text-[11px] text-neutral-400 hover:bg-neutral-200/50 hover:text-neutral-600"
        title="每次再显示 5 个会话"
        @click="showMoreSessions(TMP_GROUP_KEY)"
      >
        显示更多（还有 {{ tmpSessions.length - visibleTmpSessions.length }} 个）
        <ChevronDown class="h-3 w-3" />
      </button>

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
          @click="activateProject(p.id)"
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

    <div class="flex h-9 shrink-0 items-center justify-between border-t border-neutral-200 px-2">
      <!-- 设置入口放在侧栏底部：侧栏在所有页面常驻，所以「设置」处处可达 -->
      <button
        class="flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-neutral-500 transition-colors hover:bg-neutral-200/70 hover:text-neutral-700"
        :class="isSettingsRoute ? 'bg-neutral-200/70 text-neutral-800' : ''"
        title="设置"
        data-no-drag
        @click="openSettings"
      >
        <Settings class="h-4 w-4" />
      </button>
      <span class="pr-1 text-[11px] text-neutral-400">{{ sessionStore.sessions.filter((s) => s.origin !== 'delegation').length }} 个会话</span>
    </div>

    <!-- 文件夹右键菜单（manual 定位到鼠标坐标） -->
    <NDropdown
      trigger="manual"
      placement="bottom-start"
      :show="folderMenu.open"
      :x="folderMenu.x"
      :y="folderMenu.y"
      :options="folderMenuOptions"
      @select="onFolderMenuSelect"
      @clickoutside="closeFolderMenu"
    />

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
