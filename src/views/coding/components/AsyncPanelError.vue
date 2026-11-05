<script setup lang="ts">
// 异步面板（编辑器/终端）chunk 加载失败时的兜底：展示原因并提供重试。
// 由 defineAsyncComponent 注入 error / retry props。
import { TriangleAlert } from 'lucide-vue-next'

const props = defineProps<{
  error?: Error
  retry?: () => void
}>()
</script>

<template>
  <div class="flex h-full w-full flex-col items-center justify-center gap-2 bg-white px-6 text-center">
    <TriangleAlert class="h-6 w-6 text-amber-400" />
    <p class="text-xs text-neutral-500">模块加载失败{{ props.error ? `：${props.error.message}` : '' }}</p>
    <button
      v-if="props.retry"
      class="cursor-pointer rounded-md border border-neutral-200 bg-white px-3 py-1 text-xs text-neutral-600 hover:bg-neutral-50"
      @click="props.retry?.()"
    >
      点击重试
    </button>
  </div>
</template>
