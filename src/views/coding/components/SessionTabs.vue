<script setup lang="ts">
import { X } from 'lucide-vue-next'
import { useWorkspaceStore } from '../../../stores/workspace'
import { useSessionStore } from '../../../stores/session'

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
</script>

<template>
  <!-- Chrome 风格标签：圆角顶部、激活标签与下方内容连通、非激活标签间细分隔线 -->
  <div
    class="flex min-w-0 items-stretch self-stretch overflow-x-auto pl-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
    data-no-drag
  >
    <div
      v-for="(id, index) in workspace.openSessionIds"
      :key="id"
      class="group relative flex h-full max-w-56 min-w-24 shrink-0 cursor-pointer items-center gap-2 rounded-t-lg px-3"
      :class="[
        id === workspace.activeSessionId
          ? 'relative z-[1] bg-white text-neutral-800'
          : 'text-neutral-500 hover:bg-neutral-200/70',
        // 相邻两个非激活标签之间显示细分隔线
        index > 0 &&
        id !== workspace.activeSessionId &&
        workspace.openSessionIds[index - 1] !== workspace.activeSessionId
          ? 'border-l border-neutral-300/70'
          : '',
      ]"
      :title="sessionStore.get(id)?.title ?? id"
      @click="workspace.openSession(id)"
    >
      <!-- 状态圆点（favicon 位）：生成中蓝色脉冲 / 空闲灰 -->
      <span
        class="h-2 w-2 shrink-0 rounded-full"
        :class="workspace.isGenerating(id) ? 'animate-pulse bg-blue-500' : 'bg-neutral-400/70'"
      />
      <span class="min-w-0 flex-1 truncate text-xs" :class="id === workspace.activeSessionId && 'font-medium'">
        {{ sessionStore.get(id)?.title ?? '已删除会话' }}
      </span>
      <button
        class="flex h-4.5 w-4.5 shrink-0 cursor-pointer items-center justify-center rounded-full text-neutral-400 hover:bg-neutral-300/70 hover:text-neutral-700"
        :class="id === workspace.activeSessionId ? 'flex' : 'hidden group-hover:flex'"
        title="关闭标签"
        @click.stop="workspace.closeTab(id)"
      >
        <X class="h-3 w-3" />
      </button>
    </div>
  </div>
</template>
