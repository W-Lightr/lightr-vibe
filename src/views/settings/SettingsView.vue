<script setup lang="ts">
// 设置页全屏壳：顶部拖拽条 + 自绘窗口控件（复用 HomeView 模式）+ 左菜单 + 内容区。
// 独立路由 /settings，不嵌套 WorkspaceLayout（PRD R1）；分区切换用页内状态（design D4）。
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { ArrowLeft, Settings } from 'lucide-vue-next'
import SettingsMenu from './components/SettingsMenu.vue'
import AgentManageView from './components/AgentManageView.vue'
import WindowControls from '../coding/components/WindowControls.vue'
import type { SettingsSectionId } from '../../types/settings'

const router = useRouter()
// 浏览器 dev 环境为 null：隐藏窗口三键（AC7）


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
    <!--
      单条 header：返回 / 标题 —— 窗口三键。

      与其他页面同版式；不再自带独立的 h-10 拖拽条 + 自带窗口三键，
      窗口三键每屏只出现一次（spec: tauri-bridge.md）。
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
      <Settings class="h-4 w-4 shrink-0 text-neutral-400" />
      <h1 class="shrink-0 text-[13px] font-medium text-neutral-800">设置</h1>
      <span class="flex-1" />
      <WindowControls />
    </header>


    <!-- 左菜单 + 右内容区 -->
    <div class="flex min-h-0 flex-1">
      <SettingsMenu v-model:active-section="activeSection" />
      <main class="min-w-0 flex-1 overflow-y-auto bg-neutral-50">
        <AgentManageView v-if="activeSection === 'agent'" />
      </main>
    </div>
  </div>
</template>
