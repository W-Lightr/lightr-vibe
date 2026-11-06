<script setup lang="ts">
// 委派 MCP 工具的专用卡片。状态以 broker store 为准，工具输出仅作刷新/历史回退。
import { computed } from 'vue'
import { Bot, CircleAlert, ExternalLink, LoaderCircle, ShieldQuestion, Square } from 'lucide-vue-next'
import type { ToolCallPartData } from '../../../types/contentPart'
import { useDelegationStore } from '../../../stores/delegation'
import { useWorkspaceStore } from '../../../stores/workspace'
import type { CodexCollabInfo } from '../../../utils/codexCollab'

const props = defineProps<{ part: ToolCallPartData, nativeCollab?: CodexCollabInfo | null }>()
const delegation = useDelegationStore()
const workspace = useWorkspaceStore()

function objectFrom(text: string | undefined): Record<string, unknown> | null {
  if (!text) return null
  try { const value: unknown = JSON.parse(text); return typeof value === 'object' && value !== null ? value as Record<string, unknown> : null } catch { return null }
}

const input = computed(() => objectFrom(props.part.input))
const output = computed(() => objectFrom(props.part.result))
function taskIdFrom(value: Record<string, unknown> | null): string | null {
  if (!value) return null
  const direct = value.taskId ?? value.task_id
  if (typeof direct === 'string') return direct
  const structured = value.structuredContent
  if (typeof structured === 'object' && structured !== null) {
    const fromStructured = taskIdFrom(structured as Record<string, unknown>)
    if (fromStructured) return fromStructured
  }
  // get_delegation_status 形如 { tasks: [record] }；卡片展示首项，完整状态仍在 transcript 原文。
  const first = Array.isArray(value.tasks) ? value.tasks[0] : null
  return typeof first === 'object' && first !== null ? taskIdFrom(first as Record<string, unknown>) : null
}
const taskId = computed(() => {
  return props.nativeCollab?.agents[0]?.threadId ?? taskIdFrom(output.value)
})
const record = computed(() => delegation.get(taskId.value))
const task = computed(() => props.nativeCollab?.prompt ?? record.value?.task ?? (typeof input.value?.task === 'string' ? input.value.task : '子代理任务'))
const agent = computed(() => props.nativeCollab?.model ?? record.value?.agentType ?? (typeof input.value?.agent_type === 'string' ? input.value.agent_type : 'Agent'))
const status = computed(() => props.nativeCollab?.status ?? record.value?.status ?? (props.part.status === 'error' ? 'failed' : props.part.status === 'done' ? 'completed' : 'running'))
const statusText = computed(() => ({ running: '运行中', waiting_permission: '等待授权', waiting_question: '等待回答', completed: '已完成', failed: '失败', canceled: '已取消', interrupted: '已中断' }[status.value] ?? '运行中'))
const isWaiting = computed(() => status.value === 'waiting_permission' || status.value === 'waiting_question')
const canOpen = computed(() => !props.nativeCollab && !!record.value?.childSessionId)
const nativeMessage = computed(() => props.nativeCollab?.agents.map((agent) => agent.message).find((message): message is string => !!message) ?? null)

function openChild(): void { if (record.value?.childSessionId) workspace.openDelegatedChild(record.value.childSessionId) }
</script>

<template>
  <div class="rounded-lg border border-neutral-200 bg-white px-3 py-2.5 text-xs shadow-sm">
    <div class="flex min-w-0 items-center gap-2">
      <Bot class="h-4 w-4 shrink-0 text-blue-600" />
      <span class="min-w-0 flex-1 truncate font-medium text-neutral-700">{{ nativeCollab ? 'Codex 协作代理' : '委派给' }} {{ agent }}</span>
      <span class="rounded px-1.5 py-0.5 text-[10px]" :class="status === 'failed' ? 'bg-red-50 text-red-600' : isWaiting ? 'bg-amber-50 text-amber-700' : status === 'running' ? 'bg-blue-50 text-blue-600' : 'bg-neutral-100 text-neutral-500'">
        <LoaderCircle v-if="status === 'running'" class="mr-1 inline h-3 w-3 animate-spin" />
        <ShieldQuestion v-else-if="isWaiting" class="mr-1 inline h-3 w-3" />
        <CircleAlert v-else-if="status === 'failed'" class="mr-1 inline h-3 w-3" />
        <Square v-else class="mr-1 inline h-2.5 w-2.5" />{{ statusText }}
      </span>
    </div>
    <p class="mt-1 truncate text-neutral-500" :title="task">{{ task }}</p>
    <div class="mt-2 flex items-center justify-between gap-2">
      <span v-if="taskId" class="font-mono text-[10px] text-neutral-400">#{{ taskId.slice(0, 8) }}</span>
      <span v-else class="text-[10px] text-neutral-400">等待任务确认</span>
      <button v-if="canOpen" class="flex items-center gap-1 text-[11px] text-blue-600 hover:text-blue-700" @click="openChild">
        查看会话 <ExternalLink class="h-3 w-3" />
      </button>
    </div>
    <p v-if="nativeMessage" class="mt-1 line-clamp-2 text-[10px] text-neutral-400">{{ nativeMessage }}</p>
    <p v-else-if="nativeCollab" class="mt-1 text-[10px] text-neutral-400">Codex 原生协作状态（只读）</p>
    <p v-else-if="record?.message" class="mt-1 text-[10px] text-neutral-400">{{ record.message }}</p>
  </div>
</template>
