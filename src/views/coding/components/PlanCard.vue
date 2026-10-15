<script setup lang="ts">
// 计划卡（plan update）：只读清单 + 状态徽章。流式增量按 entries 整体覆盖（父级消费）。
// 挂在消息流底部（ConversationView），随会话 planEntries 渲染；无条目则不显示。
import { computed } from 'vue'
import { Check, Loader2, Circle } from 'lucide-vue-next'
import type { AcpPlanEntry, AcpPlanEntryStatus } from '../../../types/acp'

const props = defineProps<{ entries: AcpPlanEntry[] }>()

/** 状态中文标签（未知/缺失按待办） */
function statusLabel(status: AcpPlanEntryStatus | undefined): string {
  switch (status) {
    case 'in_progress':
      return '进行中'
    case 'completed':
      return '已完成'
    default:
      return '待办'
  }
}

/** 状态徽章样式（蓝=进行中 / 绿=完成 / 灰=待办） */
function badgeClass(status: AcpPlanEntryStatus | undefined): string {
  switch (status) {
    case 'in_progress':
      return 'bg-blue-50 text-blue-600 border-blue-200'
    case 'completed':
      return 'bg-emerald-50 text-emerald-600 border-emerald-200'
    default:
      return 'bg-neutral-100 text-neutral-500 border-neutral-200'
  }
}

const completedCount = computed(
  () => props.entries.filter((e) => e.status === 'completed').length,
)
</script>

<template>
  <div
    v-if="entries.length"
    class="mb-4 overflow-hidden rounded-xl border border-neutral-200 bg-white"
  >
    <div class="flex items-center justify-between border-b border-neutral-100 px-3.5 py-2">
      <span class="text-xs font-medium text-neutral-700">计划</span>
      <span class="text-[11px] text-neutral-400">{{ completedCount }}/{{ entries.length }}</span>
    </div>
    <ul class="divide-y divide-neutral-50">
      <li
        v-for="(entry, i) in entries"
        :key="i"
        class="flex items-start gap-2.5 px-3.5 py-2 text-[13px]"
      >
        <!-- 状态图标：进行中转圈 / 完成勾 / 待办空圈 -->
        <Loader2
          v-if="entry.status === 'in_progress'"
          class="mt-0.5 h-3.5 w-3.5 shrink-0 animate-spin text-blue-500"
        />
        <Check
          v-else-if="entry.status === 'completed'"
          class="mt-0.5 h-3.5 w-3.5 shrink-0 text-emerald-500"
        />
        <Circle v-else class="mt-0.5 h-3.5 w-3.5 shrink-0 text-neutral-300" />

        <span
          class="flex-1 leading-6"
          :class="entry.status === 'completed' ? 'text-neutral-400 line-through' : 'text-neutral-700'"
        >
          {{ entry.content }}
        </span>

        <span
          class="mt-0.5 shrink-0 rounded-md border px-1.5 py-0.5 text-[10px] font-medium"
          :class="badgeClass(entry.status)"
        >
          {{ statusLabel(entry.status) }}
        </span>
      </li>
    </ul>
  </div>
</template>
