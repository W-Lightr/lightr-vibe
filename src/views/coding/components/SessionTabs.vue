<script setup lang="ts">
import { X, FileCode2, FileText, File } from 'lucide-vue-next'
import { useWorkspaceStore } from '../../../stores/workspace'
import { useSessionStore } from '../../../stores/session'
import { useConversation, clearDraftInput } from '../../../composables/useConversation'
import { fileIconKind } from '../../../utils/file'

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
const { closeConnection } = useConversation()

/** 关闭标签：会话标签额外销毁其 ACP 连接（生成中先 cancel，再 session/close + 杀进程，
 * design §8.3「关标签 = 真正销毁该会话进程」）；
 * 先读取 tab 的 sessionId 再关，关闭侧效用 fire-and-forget（best-effort） */
function closeTab(tabId: string) {
  const tab = workspace.tabs.find((t) => t.id === tabId)
  const sessionId = tab?.kind === 'session' ? tab.sessionId : null
  workspace.closeTab(tabId)
  if (sessionId) {
    // 延迟入列（PRD R5）：草稿关标签**无条件丢弃**（不落盘、不列表、清输入暂存）
    if (sessionStore.isDraft(sessionId)) {
      sessionStore.discardDraft(sessionId)
      clearDraftInput(sessionId)
    }
    // 已入库会话保持现状：仅杀进程，不删记录
    void closeConnection(sessionId)
  }
}

function tabTitle(tabId: string): string {
  const tab = workspace.tabs.find((t) => t.id === tabId)
  if (!tab) return ''
  if (tab.kind === 'session') return sessionStore.get(tab.sessionId)?.title ?? '已删除会话'
  return tab.fileName ?? tab.filePath ?? '文件'
}

function fileIcon(name: string) {
  const kind = fileIconKind(name)
  if (kind === 'code') return FileCode2
  if (kind === 'text') return FileText
  return File
}
</script>

<template>
  <!-- Chrome 风格标签：激活标签白底圆角顶部并连通内容区；非激活标签融入灰色标签条 -->
  <!-- 左侧保留一段灰色间隔，让首个标签的圆角与标签条底色形成层次（对照设计图） -->
  <div
    class="flex min-w-0 items-stretch self-stretch overflow-x-auto pl-4 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
    data-no-drag
  >
    <div
      v-for="tab in workspace.tabs"
      :key="tab.id"
      class="group relative flex h-full max-w-60 min-w-28 shrink-0 cursor-pointer items-center gap-2.5 rounded-t-xl px-3.5"
      :class="[
        tab.id === workspace.activeTabId
          ? 'relative z-[1] bg-white text-neutral-900'
          : 'text-neutral-500 hover:bg-neutral-200/60',
      ]"
      :title="tabTitle(tab.id)"
      @click="workspace.activeTabId = tab.id"
    >
      <!-- 会话标签：状态圆点（激活高饱和蓝 / 未激活灰蓝 / 生成中脉冲）；文件标签：类型图标 + dirty 圆点 -->
      <span
        v-if="tab.kind === 'session'"
        class="h-2 w-2 shrink-0 rounded-full"
        :class="
          workspace.isGenerating(tab.sessionId)
            ? 'animate-pulse bg-blue-500'
            : tab.id === workspace.activeTabId
              ? 'bg-blue-500'
              : 'bg-slate-400'
        "
      />
      <template v-else>
        <component :is="fileIcon(tab.fileName ?? '')" class="h-3.5 w-3.5 shrink-0 text-slate-400" />
        <!-- 未保存修改：amber 语义色圆点（ui-style：amber 仅用于警示语义） -->
        <span
          v-if="workspace.isFileDirty(tab.id)"
          class="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-400"
          title="未保存的修改"
        />
      </template>

      <span
        class="min-w-0 flex-1 truncate text-[13px]"
        :class="tab.id === workspace.activeTabId ? 'font-medium text-neutral-900' : ''"
      >
        {{ tabTitle(tab.id) }}
      </span>

      <!-- 关闭按钮：始终占位（避免悬浮时标签宽度跳动），未激活标签以透明度过渡显现 -->
      <button
        class="flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center rounded transition-opacity duration-150"
        :class="
          tab.id === workspace.activeTabId
            ? 'text-neutral-700 opacity-100 hover:bg-neutral-200 hover:text-neutral-900'
            : 'text-neutral-500 opacity-0 hover:bg-neutral-300/70 hover:text-neutral-800 group-hover:opacity-100 focus-visible:opacity-100'
        "
        title="关闭标签"
        @click.stop="closeTab(tab.id)"
      >
        <X class="h-3.5 w-3.5" />
      </button>
    </div>
  </div>
</template>
