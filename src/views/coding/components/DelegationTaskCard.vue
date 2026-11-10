<script setup lang="ts">
// 子任务列表卡片：展示型组件，不依赖 transcript 的 ToolCallPartData。
import { computed } from 'vue'
import { CircleAlert, Cpu, LoaderCircle, ShieldQuestion, Square } from 'lucide-vue-next'
import type { DelegationRecord } from '../../../types/delegation'
import { useEngineStore } from '../../../stores/engine'
import { useSessionStore } from '../../../stores/session'
import { useWorkspaceStore } from '../../../stores/workspace'
import { formatDuration } from '../../../utils/time'
import { delegationStatusClass, delegationStatusText, isWaitingStatus } from '../../../utils/delegationStatus'

const props = defineProps<{ record: DelegationRecord }>()

const engineStore = useEngineStore()
const sessionStore = useSessionStore()
const workspace = useWorkspaceStore()

/** 极端恢复场景下 broker 记录仍可列出，但子会话可能已不可用 → 卡片置灰。 */
const childSession = computed(() => sessionStore.get(props.record.childSessionId))
const engineName = computed(() => engineStore.agentName(props.record.agentType))
/** 终态用 broker 的最后更新时间；运行中按当前时间算，避免显示成 0 秒。 */
const TERMINAL_STATUS = new Set(['completed', 'failed', 'canceled', 'interrupted'])
const duration = computed(() => {
  const end = TERMINAL_STATUS.has(props.record.status) ? props.record.updatedAt : Date.now()
  return formatDuration(Math.max(0, end - props.record.createdAt))
})

function openChild() {
  if (!childSession.value) return
  workspace.openDelegatedChild(props.record.childSessionId)
}
</script>

<template>
  <button
    type="button"
    class="w-full rounded-md border border-neutral-200 bg-white px-2.5 py-2 text-left transition-colors"
    :class="childSession ? 'cursor-pointer hover:bg-neutral-50' : 'cursor-not-allowed opacity-40'"
    :disabled="!childSession"
    :title="childSession ? '查看子任务详情' : '子会话已不可用'"
    @click="openChild"
  >
    <div class="flex min-w-0 items-center gap-1.5">
      <span
        class="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px]"
        :class="delegationStatusClass(record.status)"
      >
        <LoaderCircle v-if="record.status === 'running'" class="h-3 w-3 animate-spin" />
        <ShieldQuestion v-else-if="isWaitingStatus(record.status)" class="h-3 w-3" />
        <CircleAlert v-else-if="record.status === 'failed'" class="h-3 w-3" />
        <Square v-else class="h-2.5 w-2.5" />
        {{ delegationStatusText(record.status) }}
      </span>
      <span class="flex min-w-0 flex-1 items-center gap-1 text-[11px] text-neutral-500">
        <Cpu class="h-3 w-3 shrink-0 text-neutral-400" />
        <span class="truncate">{{ engineName }}</span>
      </span>
      <span class="shrink-0 font-mono text-[10px] text-neutral-400">#{{ record.taskId.slice(0, 8) }}</span>
    </div>
    <p class="mt-1.5 truncate text-xs text-neutral-700" :title="record.task">{{ record.task }}</p>
    <div class="mt-1 flex items-center justify-between gap-2 text-[10px] text-neutral-400">
      <span>耗时 {{ duration }}</span>
      <span v-if="!childSession">子会话已不可用</span>
    </div>
  </button>
</template>
