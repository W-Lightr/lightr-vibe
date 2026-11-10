<script setup lang="ts">
// 右侧栏「子任务」页签的列表：只展示当前活动会话派生的委派记录。
import { computed } from 'vue'
import { useDelegationStore } from '../../../stores/delegation'
import DelegationTaskCard from './DelegationTaskCard.vue'

const props = defineProps<{ parentSessionId: string }>()

const delegation = useDelegationStore()

/** 按创建时间倒序（filter 已返回新数组，可原地排序）。 */
const tasks = computed(() =>
  delegation.records
    .filter((record) => record.parentSessionId === props.parentSessionId)
    .sort((a, b) => b.createdAt - a.createdAt),
)
</script>

<template>
  <div class="h-full overflow-y-auto p-2.5">
    <div v-if="tasks.length" class="flex flex-col gap-2">
      <DelegationTaskCard v-for="task in tasks" :key="task.taskId" :record="task" />
    </div>
    <p v-else class="p-4 text-center text-[11px] text-neutral-400">当前会话还没有子任务</p>
  </div>
</template>
