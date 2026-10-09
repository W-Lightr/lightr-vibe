<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { MessageSquarePlus } from 'lucide-vue-next'
import TopBar from './components/TopBar.vue'
import SidebarPanel from './components/SidebarPanel.vue'
import WorkspaceHeader from './components/WorkspaceHeader.vue'
import ConversationView from './components/ConversationView.vue'
import MessageComposer from './components/MessageComposer.vue'
import FileEditorView from './components/FileEditorView.vue'
import RightPanel from './components/RightPanel.vue'
import TerminalPanel from './components/TerminalPanel.vue'
import { useWorkspaceStore } from '../../stores/workspace'
import { useSessionStore } from '../../stores/session'
import { useProjectStore } from '../../stores/project'

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
const projectStore = useProjectStore()

const activeTab = computed(() => workspace.activeTab)
const activeSession = computed(() => sessionStore.get(workspace.activeSessionId))

// 底部终端抽屉（顶栏终端按钮切换）
const terminalVisible = ref(false)

function createSession() {
  // 新会话默认挂到最近打开的项目（无项目则为无工作区会话）；cwd 绑项目根（PRD R2）
  const projectId = projectStore.recentProjects[0]?.id
  const session = sessionStore.createSession(workspace.selectedModel, {
    kind: 'folder',
    projectId,
    cwd: projectStore.get(projectId)?.rootPath,
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
const RIGHT_MAX = 560

function onPointerMove(e: PointerEvent) {
  const dx = e.clientX - startX
  if (dragging.value === 'left') {
    workspace.leftWidth = Math.min(LEFT_MAX, Math.max(LEFT_MIN, startWidth + dx))
  } else if (dragging.value === 'right') {
    workspace.rightWidth = Math.min(RIGHT_MAX, Math.max(RIGHT_MIN, startWidth - dx))
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
  window.addEventListener('pointermove', onPointerMove)
  window.addEventListener('pointerup', stopDrag)
}

onBeforeUnmount(stopDrag)

// 响应式：窗口变窄时优先收起左侧栏
const NARROW = 1280
function onResize() {
  if (window.innerWidth < NARROW) workspace.sidebarVisible = false
}
onMounted(() => {
  onResize()
  window.addEventListener('resize', onResize)
})
onBeforeUnmount(() => window.removeEventListener('resize', onResize))
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
  width: 4px;
  flex-shrink: 0;
  cursor: col-resize;
  background: transparent;
  transition: background-color 0.15s;
}
.divider:hover,
.resizing .divider {
  background: #bfdbfe;
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

      <div class="flex min-h-0 flex-1">
        <main class="flex min-w-0 flex-1 flex-col">
          <!-- 会话标签 -->
          <template v-if="activeTab?.kind === 'session' && activeSession">
            <WorkspaceHeader :session="activeSession" />
            <ConversationView :key="activeSession.id" :session="activeSession" />
            <!-- 终端抽屉 -->
            <div v-if="terminalVisible" class="h-60 shrink-0 border-t border-neutral-200">
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
  </div>
</template>
