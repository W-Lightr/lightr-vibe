<script setup lang="ts">
import { ChevronRight } from 'lucide-vue-next'
import type { Session } from '../../../types/session'

defineProps<{ session: Session | null }>()
</script>

<template>
  <div class="flex h-9 shrink-0 items-center border-b border-neutral-200 bg-white px-4">
    <!-- 面包屑 -->
    <div class="flex min-w-0 items-center gap-1 text-xs text-neutral-500">
      <span class="shrink-0">聊天模式</span>
      <ChevronRight class="h-3 w-3 shrink-0 text-neutral-300" />
      <span class="truncate font-medium text-neutral-700">{{ session?.title ?? '未选择会话' }}</span>
    </div>
  </div>
</template>
