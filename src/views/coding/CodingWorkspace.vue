<script setup lang="ts">
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted, ref } from 'vue'
import { MessageSquarePlus } from 'lucide-vue-next'
import TopBar from './components/TopBar.vue'
import SidebarPanel from './components/SidebarPanel.vue'
import WorkspaceHeader from './components/WorkspaceHeader.vue'
import ConversationView from './components/ConversationView.vue'
import MessageComposer from './components/MessageComposer.vue'
import RightPanel from './components/RightPanel.vue'
import NewSessionDialog from './components/NewSessionDialog.vue'
import EditorLoading from './components/EditorLoading.vue'
import TerminalLoading from './components/TerminalLoading.vue'
import AsyncPanelError from './components/AsyncPanelError.vue'
import { loadFileEditor, loadTerminal, prefetchFileEditor, scheduleIdle } from './lazyWorkspace'
import { useWorkspaceStore } from '../../stores/workspace'
import { useSessionStore } from '../../stores/session'
import { useProjectStore } from '../../stores/project'
import type { EngineWire } from '../../types/engine'
import type { NewSessionTarget } from '../../types/session'

// 编辑器（Monaco）与终端（xterm）合计数 MB，且首次进入并不渲染：
// 必须走异步组件，不能出现在工作区首屏 chunk 的静态依赖里。
const FileEditorView = defineAsyncComponent({
  loader: loadFileEditor,
  loadingComponent: EditorLoading,
  errorComponent: AsyncPanelError,
  delay: 0,
  timeout: 30_000,
})
const TerminalPanel = defineAsyncComponent({
  loader: loadTerminal,
  loadingComponent: TerminalLoading,
  errorComponent: AsyncPanelError,
  delay: 0,
  timeout: 30_000,
})

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
const projectStore = useProjectStore()

const activeTab = computed(() => workspace.activeTab)
const activeSession = computed(() => sessionStore.get(workspace.activeSessionId))

// 底部终端抽屉（顶栏终端按钮切换）
const terminalVisible = ref(false)

function createSession() {
  // 新会话：弹窗选引擎 + 文件夹（PRD R2），确认后创建草稿
  newDialogOpen.value = true
}

/** 弹窗确认：按所选引擎 + 目标创建草稿会话（延迟入列，PRD D3） */
const newDialogOpen = ref(false)
function onDialogConfirm(payload: { engineId: EngineWire; target: NewSessionTarget }) {
  if (payload.target.kind === 'tmp') {
    const session = sessionStore.createDraft(workspace.selectedModel, {
      kind: 'tmp',
      engineId: payload.engineId,
    })
    workspace.openSession(session.id)
    return
  }
  const project = projectStore.get(payload.target.projectId)
  const session = sessionStore.createDraft(workspace.selectedModel, {
    kind: 'folder',
    projectId: payload.target.projectId,
    cwd: project?.rootPath,
    engineId: payload.engineId,
  })
  workspace.openSession(session.id)
}

// ---- 三栏拖拽调宽（自绘分隔条；开合动画由 CSS 宽度过渡承担）----
const dragging = ref<null | 'left' | 'right'>(null)
let startX = 0
let startWidth = 0

const LEFT_MIN = 200
const LEFT_MAX = 480
const RIGHT_MIN = 260
/** 右侧面板的硬上限（px）：再大就不再是「侧栏」了 */
const RIGHT_HARD_MAX = 1600
/**
 * 主区必须保留的最小宽度（px）。
 *
 * 右栏要用来展示网页内容，固定 560px 上限远远不够，所以改成「按容器宽度
 * 取比例 + 给主区留底线」。没有这条底线时，`main` 上的 `min-w-0` 会让它被
 * 一路压到 0，右栏越宽主区越不可用。
 */
const MAIN_MIN_WIDTH = 420

/** 拖拽容器（中间那一栏）的实际可用宽度 */
const dragContainerWidth = ref(0)
const dragContainerEl = ref<HTMLElement | null>(null)

/** 右侧面板当前允许的最大宽度：容器宽度 - 主区底线，且不超过硬上限 */
function rightMaxWidth(): number {
  const container = dragContainerWidth.value
  if (container <= 0) return RIGHT_HARD_MAX
  return Math.max(RIGHT_MIN, Math.min(RIGHT_HARD_MAX, container - MAIN_MIN_WIDTH))
}

/** 同步容器宽度（挂载 + 窗口 resize 时） */
function syncContainerWidth() {
  dragContainerWidth.value = dragContainerEl.value?.clientWidth ?? 0
}

function onPointerMove(e: PointerEvent) {
  const dx = e.clientX - startX
  if (dragging.value === 'left') {
    workspace.leftWidth = Math.min(LEFT_MAX, Math.max(LEFT_MIN, startWidth + dx))
  } else if (dragging.value === 'right') {
    const max = rightMaxWidth()
    workspace.rightWidth = Math.min(max, Math.max(RIGHT_MIN, startWidth - dx))
  }
}

function stopDrag() {
  dragging.value = null
  window.removeEventListener('pointermove', onPointerMove)
  window.removeEventListener('pointerup', stopDrag)
}

function startDrag(side: 'left' | 'right', e: PointerEvent) {
  dragging.value = side
  startX = e.clientX
  startWidth = side === 'left' ? workspace.leftWidth : workspace.rightWidth
  // 左栏宽度会改变中间容器的可用宽度，本次拖拽开始前重新量一次
  syncContainerWidth()
  window.addEventListener('pointermove', onPointerMove)
  window.addEventListener('pointerup', stopDrag)
}

onBeforeUnmount(stopDrag)

// 响应式：窗口变窄时优先收起左侧栏
const NARROW = 1280
function onResize() {
  if (window.innerWidth < NARROW) workspace.sidebarVisible = false
  // 窗口变小后，之前拉宽的右栏可能已经不合法：收回底线内
  syncContainerWidth()
  const max = rightMaxWidth()
  if (workspace.rightWidth > max) workspace.rightWidth = max
}
onMounted(() => {
  onResize()
  syncContainerWidth()
  window.addEventListener('resize', onResize)
  window.addEventListener('resize', syncContainerWidth)
  // 首帧之后再空闲预取编辑器 chunk（Monaco），首次打开文件时不必等下载
  scheduleIdle(prefetchFileEditor, 3000)
})
onBeforeUnmount(() => {
  window.removeEventListener('resize', onResize)
  window.removeEventListener('resize', syncContainerWidth)
})
</script>

<style scoped>
/* 栏宽开合动画；拖拽调宽时禁用过渡以保证跟手 */
.col {
  transition: width 0.22s cubic-bezier(0.25, 0.8, 0.25, 1);
  overflow: hidden;
  flex-shrink: 0;
}
.resizing .col {
  transition: none;
}

.divider {
  position: relative;
  z-index: 10;
  width: 1px;
  flex-shrink: 0;
  cursor: col-resize;
  background: transparent;
}
/* 不可见但保留 6px 宽的拖拽命中区（居中覆盖，不占布局宽度） */
.divider::after {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  left: -3px;
  right: -3px;
}
</style>

<template>
  <div class="flex h-full bg-white" :class="dragging && 'resizing'">
    <!-- 左侧栏：顶到窗口顶部；开合宽度过渡动画 -->
    <div class="col" :style="{ width: (workspace.sidebarVisible ? workspace.leftWidth : 0) + 'px' }">
      <div class="h-full" :style="{ width: workspace.leftWidth + 'px' }">
        <SidebarPanel />
      </div>
    </div>
    <div
      v-if="workspace.sidebarVisible"
      class="divider"
      title="拖动调整左栏宽度"
      @pointerdown="startDrag('left', $event)"
    />

    <!-- 中央：标签条（横跨中央+右栏）+ 工作区 -->
    <div class="flex h-full min-w-0 flex-1 flex-col">
      <TopBar @toggle-terminal="terminalVisible = !terminalVisible" />

      <div ref="dragContainerEl" class="flex min-h-0 flex-1">
        <main class="flex min-w-0 flex-1 flex-col">
          <!-- 会话标签 -->
          <template v-if="activeTab?.kind === 'session' && activeSession">
            <WorkspaceHeader :session="activeSession" />
            <ConversationView :key="activeSession.id" :session="activeSession" />
            <!-- 终端抽屉：深色底避免 loading 占位与终端主题之间闪白 -->
            <div v-if="terminalVisible" class="h-60 shrink-0 border-t border-neutral-200 bg-neutral-900">
              <TerminalPanel />
            </div>
            <MessageComposer :session-id="activeSession.id" />
          </template>

          <!-- 文件编辑标签 -->
          <FileEditorView v-else-if="activeTab?.kind === 'file'" :key="activeTab.id" :tab="activeTab" />

          <!-- 无打开标签：空白引导 -->
          <div v-else class="flex flex-1 flex-col items-center justify-center gap-3 text-neutral-400">
            <MessageSquarePlus class="h-8 w-8 text-neutral-300" />
            <p class="text-sm">没有打开的标签</p>
            <button
              class="cursor-pointer rounded-md border border-neutral-200 bg-white px-3 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50"
              @click="createSession"
            >
              新建会话
            </button>
          </div>
        </main>

        <!-- 右侧面板：置于标签条下方（内部）；开合宽度过渡动画 -->
        <div
          v-if="workspace.inspectorVisible"
          class="divider"
          title="拖动调整右栏宽度"
          @pointerdown="startDrag('right', $event)"
        />
        <div class="col" :style="{ width: (workspace.inspectorVisible ? workspace.rightWidth : 0) + 'px' }">
          <div class="h-full" :style="{ width: workspace.rightWidth + 'px' }">
            <RightPanel :session="activeSession" />
          </div>
        </div>
      </div>
    </div>

    <!-- 新建会话弹窗（选引擎，PRD R5） -->
    <NewSessionDialog
      :open="newDialogOpen"
      @close="newDialogOpen = false"
      @confirm="onDialogConfirm"
    />
  </div>
</template>
