<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useMessage } from 'naive-ui'
import {
  Minus,
  Square,
  X,
  Plus,
  Play,
  SquareTerminal,
  Ellipsis,
  PanelRight,
  PanelLeftOpen,
  Search,
  Settings,
} from 'lucide-vue-next'
import SessionTabs from './SessionTabs.vue'
import { useWorkspaceStore } from '../../../stores/workspace'
import { useSessionStore } from '../../../stores/session'

defineEmits<{ 'toggle-terminal': [] }>()

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
const appWindow = getCurrentWindow()
const message = useMessage()

function newSession() {
  const session = sessionStore.createSession(workspace.selectedModel)
  workspace.openSession(session.id)
}

/** 尚未接入后端能力的入口：给出明确反馈，不伪造功能 */
function notImplemented(label: string) {
  message.info(`「${label}」尚未接入后端能力`)
}
</script>

<template>
  <!-- 标签条：浅灰底，仅覆盖中央+右侧区域；左侧栏独立顶到窗口顶部 -->
  <!-- 底部 1px 分隔线用 ::before 绘制，激活标签的白底可将其"切断"（Chrome 效果） -->
  <div
    data-drag-region
    class="relative flex h-10 shrink-0 items-stretch bg-neutral-100 before:absolute before:inset-x-0 before:bottom-0 before:h-px before:bg-neutral-200 before:content-['']"
  >
    <!-- 侧栏隐藏时：左缘显示展开侧栏与搜索入口 -->
    <div v-if="!workspace.sidebarVisible" class="flex shrink-0 items-center gap-0.5 pl-1.5" data-no-drag>
      <button
        class="tb"
        title="展开侧栏"
        @click="workspace.sidebarVisible = true"
      >
        <PanelLeftOpen class="h-4 w-4" />
      </button>
      <button
        class="tb"
        title="搜索会话"
        @click="
          () => {
            workspace.sidebarVisible = true
            workspace.sidebarSearchOpen = true
          }
        "
      >
        <Search class="h-4 w-4" />
      </button>
    </div>

    <!-- 会话标签 -->
    <SessionTabs />

    <!-- 新建标签 -->
    <button
      class="mx-1 flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center self-center rounded-full text-neutral-500 hover:bg-neutral-200/70"
      title="新建会话"
      data-no-drag
      @click="newSession"
    >
      <Plus class="h-4 w-4" />
    </button>

    <div class="min-w-2 flex-1" />

    <!-- 右侧功能区 + 窗口控制 -->
    <div class="flex shrink-0 items-center gap-0.5 self-center pr-1" data-no-drag>
      <button class="tb" title="运行" @click="notImplemented('运行')">
        <Play class="h-4 w-4" />
      </button>
      <button class="tb" title="在 VS Code 中打开" @click="notImplemented('VS Code 集成')">
        <svg viewBox="0 0 24 24" class="h-4 w-4" fill="#0078d4" aria-hidden="true">
          <path
            d="M23.15 2.587 18.03.659a1.5 1.5 0 0 0-1.687.312L9.44 7.353 5.18 4.12a.75.75 0 0 0-.966.05L2.255 5.99a.75.75 0 0 0 0 1.093l3.723 3.418-3.723 3.417a.75.75 0 0 0 0 1.092l1.959 1.82a.75.75 0 0 0 .966.051l4.26-3.233 6.912 6.382a1.5 1.5 0 0 0 1.687.312l5.113-1.928a1.5 1.5 0 0 0 .975-1.405V3.992a1.5 1.5 0 0 0-.976-1.405ZM18.055 18.44l-5.555-5.124 5.555-5.124v10.248Z"
          />
        </svg>
      </button>
      <button class="tb" title="终端" @click="$emit('toggle-terminal')">
        <SquareTerminal class="h-4 w-4" />
      </button>
      <button class="tb" title="更多操作" @click="notImplemented('更多操作')">
        <Ellipsis class="h-4 w-4" />
      </button>
      <button
        class="tb"
        :class="workspace.inspectorVisible && 'bg-neutral-200/80 text-neutral-700'"
        :title="workspace.inspectorVisible ? '隐藏详情面板' : '显示详情面板'"
        @click="workspace.inspectorVisible = !workspace.inspectorVisible"
      >
        <PanelRight class="h-4 w-4" />
      </button>
      <button class="tb" title="设置" @click="notImplemented('设置')">
        <Settings class="h-4 w-4" />
      </button>

      <div class="mx-1 h-4 w-px bg-neutral-300" />

      <!-- 窗口控制（无边框自绘） -->
      <button
        class="tb"
        title="最小化"
        @click="appWindow.minimize()"
      >
        <Minus class="h-4 w-4" />
      </button>
      <button
        class="tb"
        title="最大化 / 还原"
        @click="appWindow.toggleMaximize()"
      >
        <Square class="h-3.5 w-3.5" />
      </button>
      <button
        class="tb cursor-pointer hover:bg-red-500 hover:text-white"
        title="关闭"
        @click="appWindow.close()"
      >
        <X class="h-4 w-4" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.tb {
  display: flex;
  height: 2rem;
  width: 2rem;
  cursor: pointer;
  align-items: center;
  justify-content: center;
  border-radius: 0.375rem;
  color: #525252;
}
.tb:hover {
  background-color: #e5e5e5;
  color: #262626;
}
</style>
