<script setup lang="ts">
// 运行历史弹层：某条自动化的运行记录（倒序）。
//
// `enqueue_task` 的运行可跳到任务看板；`launch_session` 的运行显示会话 id。
// 在飞（running）的记录提供「取消」。
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { ExternalLink, LoaderCircle, Square, X } from 'lucide-vue-next'
import { formatDateTime, formatDuration, formatRelativeTime } from '../../../utils/time'
import {
  automationErrorMessage,
  RUN_STATUS_LABELS,
  RUN_STATUS_TYPES,
} from '../../../types/automation'
import type { Automation, AutomationRun } from '../../../types/automation'

const props = defineProps<{
  open: boolean
  automation: Automation | null
  /** 由父级提供的读取入口（父级经 store → service） */
  loadRuns: (id: string) => Promise<AutomationRun[]>
  /** 由父级提供的取消入口；成功返回、失败抛出 */
  cancelRun: (automationId: string, runId: string) => Promise<void>
}>()

const emit = defineEmits<{
  close: []
  /**
   * 取消某条在飞运行。
   *
   * 本组件**不**直接调 service：写路径统一由父级的 store 承担
   * （state-management spec 的分层边界），父级取消成功后会刷新列表，
   * 本弹层通过 props 变化/事件重拉历史。
   */
  cancel: [runId: string]
}>()

const router = useRouter()

const rows = ref<AutomationRun[]>([])
const loading = ref(false)
const errorText = ref<string | null>(null)
const canceling = ref<string | null>(null)

watch(
  () => [props.open, props.automation?.id] as const,
  async ([open, id]) => {
    if (!open || !id) {
      rows.value = []
      return
    }
    loading.value = true
    errorText.value = null
    try {
      rows.value = await props.loadRuns(id)
    } catch (err) {
      errorText.value = automationErrorMessage(err)
      rows.value = []
    } finally {
      loading.value = false
    }
  },
  { immediate: true },
)

const title = computed(() => props.automation?.name ?? '运行历史')

function duration(run: AutomationRun): string {
  if (run.startedAt === null) return '—'
  const end = run.endedAt ?? Date.now()
  return formatDuration(end - run.startedAt)
}

function triggerText(run: AutomationRun): string {
  return run.trigger === 'schedule' ? '定时' : '手动'
}

async function cancel(run: AutomationRun) {
  if (canceling.value || !props.automation) return
  canceling.value = run.id
  try {
    await props.cancelRun(props.automation.id, run.id)
    // 就地更新，避免整表刷新带来的闪烁
    const row = rows.value.find((r) => r.id === run.id)
    if (row) {
      row.status = 'cancelled'
      row.endedAt = Date.now()
      row.stopReason = 'cancelled'
    }
  } catch (err) {
    errorText.value = automationErrorMessage(err)
  } finally {
    canceling.value = null
  }
}

/** 跳到该运行建出的任务：看板上按项目筛选 */
function openTask() {
  if (!props.automation) return
  void router.push(`/tasks/${props.automation.projectId}`)
  emit('close')
}
</script>

<template>
  <div
    v-if="props.open"
    data-no-drag
    class="absolute inset-0 z-50 flex items-center justify-center bg-neutral-900/20"
    @mousedown.self="emit('close')"
  >
    <div class="flex max-h-[80vh] w-[720px] flex-col rounded-lg border border-neutral-200 bg-white">
      <div class="flex shrink-0 items-center justify-between px-5 pt-4 pb-3">
        <span class="min-w-0 truncate text-[14px] font-medium text-neutral-800">
          运行历史 · {{ title }}
        </span>
        <button
          class="flex h-6 w-6 shrink-0 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
          title="关闭"
          @click="emit('close')"
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto px-5 pb-4">
        <p v-if="errorText" class="mb-2 rounded-md bg-red-50 px-2.5 py-2 text-[11px] text-red-700">
          {{ errorText }}
        </p>

        <p v-if="loading" class="py-6 text-center text-[12px] text-neutral-400">加载中…</p>
        <p
          v-else-if="rows.length === 0"
          class="py-8 text-center text-[12px] text-neutral-400"
        >
          还没有运行记录
        </p>

        <div v-else class="space-y-1.5">
          <div
            v-for="run in rows"
            :key="run.id"
            class="rounded-md border border-neutral-200 px-2.5 py-2"
          >
            <div class="flex min-w-0 items-center gap-1.5">
              <span
                class="shrink-0 rounded px-1 py-px text-[10px]"
                :class="{
                  'bg-neutral-100 text-neutral-500': RUN_STATUS_TYPES[run.status] === 'default',
                  'bg-emerald-50 text-emerald-600': RUN_STATUS_TYPES[run.status] === 'success',
                  'bg-red-50 text-red-600': RUN_STATUS_TYPES[run.status] === 'error',
                  'bg-amber-50 text-amber-600': RUN_STATUS_TYPES[run.status] === 'warning',
                  'bg-blue-50 text-blue-600': RUN_STATUS_TYPES[run.status] === 'info',
                }"
              >
                {{ RUN_STATUS_LABELS[run.status] }}
              </span>
              <span class="shrink-0 text-[10px] text-neutral-500">{{ triggerText(run) }}</span>
              <span class="shrink-0 text-[10px] tabular-nums text-neutral-400">
                {{ formatDateTime(run.createdAt) }}
              </span>
              <span class="shrink-0 text-[10px] tabular-nums text-neutral-400">
                · {{ duration(run) }}
              </span>
              <span class="flex-1" />
              <button
                v-if="run.status === 'running'"
                class="flex h-5 cursor-pointer items-center gap-1 rounded border border-neutral-200 px-1.5 text-[10px] text-neutral-600 hover:bg-neutral-50 disabled:opacity-40"
                :disabled="canceling === run.id"
                @click="cancel(run)"
              >
                <LoaderCircle v-if="canceling === run.id" class="h-2.5 w-2.5 animate-spin" />
                <Square v-else class="h-2 w-2" />
                取消
              </button>
              <button
                v-if="run.taskId"
                class="flex h-5 cursor-pointer items-center gap-1 rounded border border-neutral-200 px-1.5 text-[10px] text-neutral-600 hover:bg-neutral-50"
                @click="openTask()"
              >
                <ExternalLink class="h-2.5 w-2.5" />
                看板
              </button>
            </div>

            <p v-if="run.summary" class="mt-1 line-clamp-2 text-[11px] leading-4 text-neutral-600">
              {{ run.summary }}
            </p>
            <p v-if="run.error" class="mt-1 line-clamp-2 text-[11px] leading-4 text-red-600">
              {{ run.error }}
            </p>
            <p
              v-if="run.sessionId"
              class="mt-1 truncate font-mono text-[10px] text-neutral-400"
              :title="run.sessionId"
            >
              会话 {{ run.sessionId }}
            </p>
            <p
              v-if="run.scheduledFor"
              class="mt-0.5 text-[10px] text-neutral-400"
            >
              计划于 {{ formatDateTime(run.scheduledFor) }} · 实际
              {{ run.startedAt ? formatRelativeTime(run.startedAt) : '—' }}
            </p>
          </div>
        </div>
      </div>

      <div class="flex shrink-0 justify-end border-t border-neutral-100 px-5 py-3">
        <button
          class="h-7 cursor-pointer rounded-md border border-neutral-200 px-3 text-[12px] text-neutral-600 hover:bg-neutral-50"
          @click="emit('close')"
        >
          关闭
        </button>
      </div>
    </div>
  </div>
</template>
