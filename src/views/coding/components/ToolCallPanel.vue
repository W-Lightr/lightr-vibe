<script setup lang="ts">
import { ref } from 'vue'
import { ChevronRight, Wrench, LoaderCircle, CheckCircle2, XCircle } from 'lucide-vue-next'
import type { ToolCall } from '../../../types/session'

defineProps<{ toolCalls: ToolCall[] }>()

// 默认折叠，避免占据过多空间
const expanded = ref(false)
</script>

<template>
  <div class="rounded-md border border-neutral-200 bg-neutral-50/60">
    <button
      class="flex w-full cursor-pointer items-center gap-1.5 px-2.5 py-1.5 text-xs text-neutral-600 hover:bg-neutral-100"
      @click="expanded = !expanded"
    >
      <ChevronRight class="h-3.5 w-3.5 shrink-0 text-neutral-400 transition-transform" :class="expanded && 'rotate-90'" />
      <Wrench class="h-3.5 w-3.5 shrink-0 text-neutral-500" />
      <span>工具调用 {{ toolCalls.length }} 次</span>
    </button>

    <div v-if="expanded" class="border-t border-neutral-200 px-2.5 py-2">
      <div v-for="tc in toolCalls" :key="tc.id" class="mb-2 last:mb-0">
        <div class="flex items-center gap-1.5 text-xs">
          <LoaderCircle v-if="tc.status === 'running'" class="h-3.5 w-3.5 animate-spin text-blue-500" />
          <CheckCircle2 v-else-if="tc.status === 'done'" class="h-3.5 w-3.5 text-green-600" />
          <XCircle v-else class="h-3.5 w-3.5 text-red-500" />
          <code class="rounded bg-neutral-200/70 px-1.5 py-0.5 font-mono text-[11px] text-neutral-700">{{ tc.name }}</code>
          <span class="truncate text-neutral-500">{{ tc.input }}</span>
        </div>
        <pre
          v-if="tc.result"
          class="mt-1.5 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-md border border-neutral-200 bg-white p-2 font-mono text-[11px] leading-5 text-neutral-600"
        >{{ tc.result }}</pre>
      </div>
    </div>
  </div>
</template>
