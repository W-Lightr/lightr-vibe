<script setup lang="ts">
// 设置页全屏壳：顶部拖拽条 + 自绘窗口控件（复用 HomeView 模式）+ 左菜单 + 内容区。
// 独立路由 /settings，不嵌套 WorkspaceLayout（PRD R1）；分区切换用页内状态（design D4）。
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { ArrowLeft, Minus, Square, X } from 'lucide-vue-next'
import SettingsMenu from './components/SettingsMenu.vue'
import AgentManageView from './components/AgentManageView.vue'
import { getAppWindow } from '../../utils/window'
import type { SettingsSectionId } from '../../types/settings'

const router = useRouter()
// 浏览器 dev 环境为 null：隐藏窗口三键（AC7）
const appWindow = getAppWindow()

/** 当前分区：本期仅 Agent 管理真实可用，其余菜单项为禁用占位（design D4） */
const activeSection = ref<SettingsSectionId>('agent')

/** 返回上一页；无历史（直接打开 /settings）时兜底回首页 */
function goBack() {
  if (window.history.state?.back) router.back()
  else router.push('/')
}
</script>

<template>
  <div class="flex h-full flex-col bg-white">
    <!-- 顶部拖拽条：左返回按钮，右窗口三键（无边框窗口，浏览器环境隐藏） -->
    <div data-drag-region class="flex h-10 shrink-0 items-center justify-between border-b border-neutral-200 px-2">
      <div class="flex items-center gap-1.5" data-no-drag>
        <button
          class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-100"
          title="返回"
          @click="goBack"
        >
          <ArrowLeft class="h-4 w-4" />
        </button>
        <span class="text-[13px] font-medium text-neutral-800">设置</span>
      </div>
      <div v-if="appWindow" class="flex items-center gap-0.5" data-no-drag>
        <button
          class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-100"
          title="最小化"
          @click="appWindow.minimize()"
        >
          <Minus class="h-4 w-4" />
        </button>
        <button
          class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-100"
          title="最大化 / 还原"
          @click="appWindow.toggleMaximize()"
        >
          <Square class="h-3.5 w-3.5" />
        </button>
        <button
          class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-red-500 hover:text-white"
          title="关闭"
          @click="appWindow.close()"
        >
          <X class="h-4 w-4" />
        </button>
      </div>
    </div>

    <!-- 左菜单 + 右内容区 -->
    <div class="flex min-h-0 flex-1">
      <SettingsMenu v-model:active-section="activeSection" />
      <main class="min-w-0 flex-1 overflow-y-auto bg-neutral-50">
        <AgentManageView v-if="activeSection === 'agent'" />
      </main>
    </div>
  </div>
</template>
