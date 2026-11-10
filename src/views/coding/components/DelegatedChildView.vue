<script setup lang="ts">
// 子任务详情：右栏「子任务」页签内的子会话查看器（由原 CodingWorkspace 抽屉抽出）。
import { ArrowLeft, X } from 'lucide-vue-next'
import type { Session } from '../../../types/session'
import { useWorkspaceStore } from '../../../stores/workspace'
import ConversationView from './ConversationView.vue'
import MessageComposer from './MessageComposer.vue'

defineProps<{ session: Session }>()

const workspace = useWorkspaceStore()
</script>

<template>
  <div class="flex h-full min-h-0 flex-col bg-white">
    <div class="flex shrink-0 items-center gap-1 border-b border-neutral-200 px-2 py-1.5">
      <button
        class="flex h-7 cursor-pointer items-center gap-1 rounded-md px-1.5 text-[11px] text-neutral-500 hover:bg-neutral-100 hover:text-neutral-700"
        title="返回子任务列表"
        @click="workspace.closeDelegatedChild()"
      >
        <ArrowLeft class="h-3.5 w-3.5" />
        返回
      </button>
      <span class="min-w-0 flex-1 truncate text-xs font-medium text-neutral-700" :title="`子代理 · ${session.title}`">
        子代理 · {{ session.title }}
      </span>
      <button
        class="flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-neutral-400 hover:bg-neutral-100 hover:text-neutral-700"
        title="关闭右侧栏"
        @click="workspace.closeInspector()"
      >
        <X class="h-3.5 w-3.5" />
      </button>
    </div>

    <ConversationView :key="session.id" :session="session" />
    <!-- 子会话的权限/提问队列以 local connId 过滤，复用 composer 仅提供
         交互入口；用户不必离开父会话即可处理 child 的阻塞请求。 -->
    <MessageComposer :session-id="session.id" passive />
  </div>
</template>
