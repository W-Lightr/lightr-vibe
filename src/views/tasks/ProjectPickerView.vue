<script setup lang="ts">
// 待办任务项目选择页（/tasks，prd R1/R5）：按项目铺开的卡片 + 搜索。
// 点卡片进该项目看板（/tasks/:projectId），「全部项目」卡片固定排最后（/tasks/all）。
// 数据：项目来自 projectStore，任务计数由 taskStore.tasks 在页面内聚合一次
// （避免每张卡各自过滤）；进入页面 await taskStore.load()（幂等）。
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useMessage } from 'naive-ui'
import {
  ArrowLeft,
  FolderOpen,
  FolderPlus,
  LayoutGrid,
  ListTodo,
  Plus,
  Search,
} from 'lucide-vue-next'
import WindowControls from '../coding/components/WindowControls.vue'
import TaskEditorDialog from './components/TaskEditorDialog.vue'
import { BOARD_COLUMN_IDS, columnForStatus, columnLabel } from './boardColumns'
import { useProjectStore } from '../../stores/project'
import { useTaskStore } from '../../stores/task'
import { canPickFolder, fsService, projectNameFromPath } from '../../services/fsService'
import { taskErrorMessage } from '../../types/task'
import type { BoardColumnId } from './boardColumns'
import type { EngineWire } from '../../types/engine'

/** 「全部项目」卡片的保留 id（与路由 /tasks/all 一致） */
const ALL_PROJECTS_ID = 'all'

const router = useRouter()
const message = useMessage()
const projectStore = useProjectStore()
const taskStore = useTaskStore()

const query = ref('')

/** 返回编码工作区。
 *  不用 history.back()：历史里的上一页可能是首页（应用从 `/` 启动后跳到编码区的），
 *  那样会把用户甩回首页，而不是他实际来的编码工作区。固定去 /coding。 */
function goBack(): void {
  void router.push('/coding')
}

/** 原生目录对话框仅桌面端可用（与侧栏「打开文件夹」同一能力判定） */
const pickAvailable = canPickFolder()
const picking = ref(false)

// ---- 计数聚合（一次遍历，卡片只读） ----

function emptyCounts(): Record<BoardColumnId, number> {
  return { todo: 0, inProgress: 0, attention: 0, done: 0 }
}

/** projectId → 四列计数（key 缺省 = 该项目没有任务） */
const countsByProject = computed(() => {
  const map = new Map<string, Record<BoardColumnId, number>>()
  for (const task of taskStore.tasks) {
    let counts = map.get(task.projectId)
    if (!counts) {
      counts = emptyCounts()
      map.set(task.projectId, counts)
    }
    counts[columnForStatus(task.status)] += 1
  }
  return map
})

function countsOf(projectId: string): Record<BoardColumnId, number> {
  return countsByProject.value.get(projectId) ?? emptyCounts()
}

function totalOf(counts: Record<BoardColumnId, number>): number {
  return BOARD_COLUMN_IDS.reduce((sum, id) => sum + counts[id], 0)
}

/** 计数行用的列定义（0 值灰显） */
const countColumns = computed(() =>
  BOARD_COLUMN_IDS.map((id) => ({ id, label: columnLabel(id) })),
)

// ---- 搜索（项目名 / 根路径，大小写不敏感） ----

const keyword = computed(() => query.value.trim().toLowerCase())

const filteredProjects = computed(() => {
  const q = keyword.value
  if (!q) return projectStore.projects
  return projectStore.projects.filter(
    (p) => p.name.toLowerCase().includes(q) || p.rootPath.toLowerCase().includes(q),
  )
})

const noProjects = computed(() => projectStore.projects.length === 0)

// ---- 导航 ----

function openProject(projectId: string): void {
  // 'all' 是「全部项目」视图的保留 id：若真实项目恰好是它，卡片会指向全部项目看板
  if (projectId === ALL_PROJECTS_ID) return
  projectStore.touchProject(projectId)
  void router.push(`/tasks/${projectId}`)
}

function openAll(): void {
  void router.push(`/tasks/${ALL_PROJECTS_ID}`)
}

/** 「打开文件夹」：原生对话框选目录 → 校验可读 → 加入项目列表（与侧栏同一链路） */
async function openFolder(): Promise<void> {
  if (!pickAvailable || picking.value) return
  picking.value = true
  try {
    const picked = await fsService.pickFolder()
    if (!picked) return // 用户取消
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

// ---- 新建任务（卡片页不预选项目，prd R4） ----

const editorOpen = ref(false)
/** 正在提交（防双击） */
const submitting = ref(false)

function openCreate(): void {
  editorOpen.value = true
}

async function onEditorSubmit(payload: {
  projectId: string
  title: string
  prompt: string
  engineId: EngineWire
  modelId: string | null
  effortId: string | null
  effortValue: string | null
  useWorktree: boolean
  baseBranch: string | null
  initCommand: string | null
}): Promise<void> {
  if (submitting.value) return
  submitting.value = true
  try {
    await taskStore.create({
      projectId: payload.projectId,
      title: payload.title,
      prompt: payload.prompt,
      engineId: payload.engineId,
      modelId: payload.modelId,
      effortId: payload.effortId,
      effortValue: payload.effortValue,
      useWorktree: payload.useWorktree,
      baseBranch: payload.baseBranch,
      initCommand: payload.initCommand,
    })
    editorOpen.value = false
  } catch (err) {
    message.error(taskErrorMessage(err))
  } finally {
    submitting.value = false
  }
}

// ---- 生命周期 ----

onMounted(async () => {
  if (!projectStore.loaded) {
    try {
      await projectStore.load()
    } catch {
      // 项目索引读取失败：仍可显示空状态与「打开文件夹」
    }
  }
  if (!taskStore.loaded) await taskStore.load()
})
</script>

<template>
  <div class="flex h-full flex-col bg-white">
    <!--
      单条 header：返回 / 标题 —— 搜索 —— 全部项目 · 新建 · 窗口三键。

      整条都是拖拽区（内部交互元素各自带 data-no-drag），窗口控件与页面标题
      同处一层，不再被一条空横条切成上下两块。
    -->
    <header
      data-drag-region
      class="flex h-12 shrink-0 items-center gap-2 border-b border-neutral-200 pr-2 pl-4"
    >
      <button
        class="flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
        title="返回编码工作区"
        data-no-drag
        @click="goBack"
      >
        <ArrowLeft class="h-4 w-4" />
      </button>
      <ListTodo class="h-4 w-4 shrink-0 text-neutral-400" />
      <h1 class="shrink-0 text-[13px] font-medium text-neutral-800">待办任务</h1>
      <div class="relative w-[240px]" data-no-drag>
        <Search
          class="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-neutral-400"
        />
        <input
          v-model="query"
          placeholder="搜索项目…"
          class="h-8 w-full rounded-md border border-neutral-200 bg-white pr-2 pl-7 text-[12px] text-neutral-700 outline-none placeholder:text-neutral-400 focus:border-blue-400"
        />
      </div>
      <span class="flex-1" />
      <button
        class="flex h-7 shrink-0 cursor-pointer items-center gap-1.5 rounded-full border border-neutral-200 px-3 text-[12px] text-neutral-600 transition-colors hover:bg-neutral-100"
        type="button"
        data-no-drag
        @click="openAll"
      >
        <LayoutGrid class="h-3.5 w-3.5" />
        全部项目
      </button>
      <button
        class="flex h-7 shrink-0 cursor-pointer items-center gap-1 rounded-full bg-neutral-900 px-3 text-[12px] text-white transition-colors hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-40"
        :disabled="noProjects"
        data-no-drag
        @click="openCreate"
      >
        <Plus class="h-3.5 w-3.5" />
        新建任务
      </button>
      <WindowControls />
    </header>

    <main class="min-h-0 flex-1 overflow-y-auto bg-white p-4">
      <!-- 没有任何项目：引导打开文件夹 -->
      <div v-if="noProjects" class="flex h-full flex-col items-center justify-center gap-2 text-center">
        <p class="text-xs text-neutral-400">
          {{ pickAvailable ? '还没有打开任何文件夹' : '还没有打开任何文件夹（桌面端可打开）' }}
        </p>
        <button
          class="flex h-7 cursor-pointer items-center gap-1.5 rounded-md border border-neutral-200 px-2.5 text-[12px] text-neutral-600 transition-colors hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="!pickAvailable || picking"
          @click="openFolder"
        >
          <FolderPlus class="h-3.5 w-3.5" />
          打开文件夹
        </button>
      </div>

      <!-- 搜索无匹配 -->
      <div v-else-if="filteredProjects.length === 0" class="flex h-full items-center justify-center">
        <p class="text-xs text-neutral-400">没有匹配的项目</p>
      </div>

      <div v-else class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
        <button
          v-for="project in filteredProjects"
          :key="project.id"
          class="flex cursor-pointer flex-col rounded-xl border border-neutral-200 bg-white p-3 text-left transition-colors hover:border-blue-400"
          type="button"
          :title="project.rootPath"
          @click="openProject(project.id)"
        >
          <div class="flex min-w-0 items-center gap-1.5">
            <FolderOpen class="h-4 w-4 shrink-0 text-neutral-400" />
            <span class="min-w-0 flex-1 truncate text-[13px] font-medium text-neutral-800">
              {{ project.name }}
            </span>
          </div>
          <span class="mt-2 text-[11px] text-neutral-500">
            {{ totalOf(countsOf(project.id)) }} 个任务
          </span>
          <div class="mt-1 flex flex-wrap items-center gap-x-1.5 gap-y-0.5 text-[10px]">
            <template v-for="(col, index) in countColumns" :key="col.id">
              <span :class="countsOf(project.id)[col.id] > 0 ? 'text-neutral-600' : 'text-neutral-400'">
                {{ col.label }} {{ countsOf(project.id)[col.id] }}
              </span>
              <span v-if="index < countColumns.length - 1" class="text-neutral-300">·</span>
            </template>
          </div>
        </button>
      </div>
    </main>

    <!-- 新建任务：卡片页不预选项目（prd R4） -->
    <TaskEditorDialog
      :open="editorOpen"
      :task="null"
      :default-project-id="null"
      @close="editorOpen = false"
      @submit="onEditorSubmit"
    />
  </div>
</template>
