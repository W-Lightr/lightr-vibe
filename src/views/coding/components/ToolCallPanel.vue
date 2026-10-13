<script setup lang="ts">
import { ref } from 'vue'
import { ChevronRight, LoaderCircle, CheckCircle2, XCircle } from 'lucide-vue-next'
import type { ToolCall } from '../../../types/session'

defineProps<{ toolCalls: ToolCall[] }>()

// 默认折叠；展开后逐条内联展示（紧凑行，非大卡片）
const expanded = ref(false)
</script>

<template>
  <div>
    <!-- 内联摘要行：「N 个工具」+ 折叠箭头（悬浮反馈） -->
    <button
      class="flex cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-0.5 text-xs text-neutral-500 hover:bg-neutral-100 hover:text-neutral-700"
      @click="expanded = !expanded"
    >
      <ChevronRight
        class="h-3.5 w-3.5 shrink-0 text-neutral-400 transition-transform"
        :class="expanded && 'rotate-90'"
      />
      <span>{{ toolCalls.length }} 个工具</span>
    </button>

    <!-- 展开：每条工具调用一紧凑行（状态图标 + 名称 + 摘要），结果用轻量 pre -->
    <div v-if="expanded" class="mt-1 ml-5 flex flex-col gap-1 border-l-2 border-neutral-200 pl-3">
      <div v-for="tc in toolCalls" :key="tc.id">
        <div class="flex items-center gap-1.5 text-xs leading-5">
          <LoaderCircle v-if="tc.status === 'running'" class="h-3.5 w-3.5 shrink-0 animate-spin text-blue-500" />
          <CheckCircle2 v-else-if="tc.status === 'done'" class="h-3.5 w-3.5 shrink-0 text-green-600" />
          <XCircle v-else class="h-3.5 w-3.5 shrink-0 text-red-500" />
          <code class="shrink-0 rounded bg-neutral-200/70 px-1.5 py-0.5 font-mono text-[11px] text-neutral-700">{{ tc.name }}</code>
          <span class="min-w-0 flex-1 truncate text-neutral-500">{{ tc.input }}</span>
        </div>
        <pre
          v-if="tc.result"
          class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-md border border-neutral-200 bg-neutral-50/60 p-2 font-mono text-[11px] leading-5 text-neutral-500"
        >{{ tc.result }}</pre>
      </div>
    </div>
  </div>
</template>
