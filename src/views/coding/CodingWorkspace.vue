<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { MessageSquarePlus } from 'lucide-vue-next'
import TopBar from './components/TopBar.vue'
import SidebarPanel from './components/SidebarPanel.vue'
import WorkspaceHeader from './components/WorkspaceHeader.vue'
import ConversationView from './components/ConversationView.vue'
import MessageComposer from './components/MessageComposer.vue'
import SessionInspector from './components/SessionInspector.vue'
import TerminalPanel from './components/TerminalPanel.vue'
import { useWorkspaceStore } from '../../stores/workspace'
import { useSessionStore } from '../../stores/session'

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()

const activeSession = computed(() => sessionStore.get(workspace.activeSessionId))

// 底部终端抽屉（顶栏终端按钮切换）
const terminalVisible = ref(false)

function createSession() {
  const session = sessionStore.createSession(workspace.selectedModel)
  workspace.openSession(session.id)
}

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
/* 侧栏开合动画：宽度 + 位移同步过渡 */
.sidebar-enter-active,
.sidebar-leave-active {
  transition:
    width 0.22s cubic-bezier(0.25, 0.8, 0.25, 1),
    transform 0.22s cubic-bezier(0.25, 0.8, 0.25, 1);
  overflow: hidden;
}
.sidebar-enter-from,
.sidebar-leave-to {
  width: 0 !important;
  transform: translateX(-16px);
  opacity: 0.4;
}
.sidebar-enter-to,
.sidebar-leave-from {
  opacity: 1;
}

/* 右侧详情面板开合动画：与侧栏同节奏，方向相反 */
.inspector-enter-active,
.inspector-leave-active {
  transition:
    width 0.22s cubic-bezier(0.25, 0.8, 0.25, 1),
    transform 0.22s cubic-bezier(0.25, 0.8, 0.25, 1);
  overflow: hidden;
}
.inspector-enter-from,
.inspector-leave-to {
  width: 0 !important;
  transform: translateX(16px);
  opacity: 0.4;
}
.inspector-enter-to,
.inspector-leave-from {
  opacity: 1;
}
</style>

<template>
  <div class="flex h-full bg-white">
    <!-- 左侧栏：独立成列，顶到窗口顶部；开合带滑动动画 -->
    <Transition name="sidebar">
      <SidebarPanel v-if="workspace.sidebarVisible" />
    </Transition>

    <!-- 中央 + 右侧：标签条仅覆盖此区域 -->
    <div class="flex min-w-0 flex-1 flex-col">
      <TopBar @toggle-terminal="terminalVisible = !terminalVisible" />

      <div class="flex min-h-0 flex-1">
        <!-- 中央工作区 -->
        <main class="flex min-w-0 flex-1 flex-col">
          <WorkspaceHeader :session="activeSession" />

          <template v-if="activeSession">
            <ConversationView :key="activeSession.id" :session="activeSession" />
            <!-- 终端抽屉 -->
            <div v-if="terminalVisible" class="h-60 shrink-0 border-t border-neutral-200">
              <TerminalPanel />
            </div>
            <MessageComposer :session-id="activeSession.id" />
          </template>

          <!-- 无打开标签：空白引导 -->
          <div v-else class="flex flex-1 flex-col items-center justify-center gap-3 text-neutral-400">
            <MessageSquarePlus class="h-8 w-8 text-neutral-300" />
            <p class="text-sm">没有打开的会话</p>
            <button
              class="cursor-pointer rounded-md border border-neutral-200 bg-white px-3 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50"
              @click="createSession"
            >
              新建会话
            </button>
          </div>
        </main>

        <!-- 右侧详情面板：开合带滑动动画 -->
        <Transition name="inspector">
          <SessionInspector v-if="workspace.inspectorVisible && activeSession" :session="activeSession" />
        </Transition>
      </div>
    </div>
  </div>
</template>
