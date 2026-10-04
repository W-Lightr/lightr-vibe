<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { Copy, Check, LoaderCircle } from 'lucide-vue-next'
import type { Session } from '../../../types/session'
import TokenUsagePanel from './TokenUsagePanel.vue'
import { formatDateTime } from '../../../utils/time'
import { useWorkspaceStore } from '../../../stores/workspace'

const props = defineProps<{ session: Session }>()

const workspace = useWorkspaceStore()

const copiedKey = ref<string | null>(null)

// 会话耗时：生成中每秒刷新，空闲时定格
const now = ref(Date.now())
let timer: ReturnType<typeof setInterval> | null = null

function syncTimer() {
  const generating = workspace.isGenerating(props.session.id)
  if (generating && !timer) timer = setInterval(() => (now.value = Date.now()), 1000)
  if (!generating && timer) {
    clearInterval(timer)
    timer = null
    now.value = Date.now()
  }
}

watch(
  () => workspace.isGenerating(props.session.id),
  () => syncTimer(),
  { immediate: true },
)
onBeforeUnmount(() => timer && clearInterval(timer))

const durationMs = computed(() => {
  const end = workspace.isGenerating(props.session.id) ? now.value : props.session.updatedAt
  return end - props.session.createdAt
})

const statusLabel = computed(() => {
  if (workspace.isGenerating(props.session.id)) return '生成中'
  if (props.session.status === 'error') return '异常'
  return '空闲'
})
const statusClass = computed(() => {
  if (workspace.isGenerating(props.session.id)) return 'bg-blue-50 text-blue-600'
  if (props.session.status === 'error') return 'bg-red-50 text-red-500'
  return 'bg-neutral-100 text-neutral-500'
})

const meta = computed(() => [
  { key: 'id', label: '会话 ID', value: props.session.id },
  { key: 'model', label: '当前模型', value: props.session.model },
  { key: 'branch', label: 'Git 分支', value: props.session.gitBranch },
  { key: 'ext', label: '扩展 ID', value: props.session.extensionId },
])

async function copyValue(key: string, value: string) {
  try {
    await navigator.clipboard.writeText(value)
    copiedKey.value = key
    setTimeout(() => (copiedKey.value = null), 1500)
  } catch {
    /* 剪贴板不可用时静默忽略 */
  }
}
</script>

<template>
  <aside class="flex h-full w-[335px] shrink-0 flex-col border-l border-neutral-200 bg-white">
    <div class="min-h-0 flex-1 overflow-y-auto p-4">
      <!-- 会话头部 -->
      <div class="flex items-start justify-between gap-2">
        <div class="min-w-0">
          <h2 class="truncate text-sm font-semibold text-neutral-800" :title="session.title">
            {{ session.title }}
          </h2>
          <p class="mt-1 truncate text-[11px] text-neutral-400">{{ session.model }}</p>
        </div>
        <span class="flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[11px]" :class="statusClass">
          <LoaderCircle
            v-if="workspace.isGenerating(session.id)"
            class="h-3 w-3 animate-spin"
          />
          {{ statusLabel }}
        </span>
      </div>

      <!-- 会话元数据 -->
      <div class="mt-4 grid grid-cols-2 gap-x-3 gap-y-2.5">
        <div v-for="item in meta" :key="item.key" class="min-w-0">
          <div class="text-[11px] text-neutral-400">{{ item.label }}</div>
          <div class="mt-0.5 flex items-center gap-1">
            <span class="truncate font-mono text-[11px] text-neutral-700" :title="item.value">
              {{ item.value }}
            </span>
            <button
              class="shrink-0 cursor-pointer text-neutral-300 hover:text-neutral-500"
              title="复制完整值"
              @click="copyValue(item.key, item.value)"
            >
              <Check v-if="copiedKey === item.key" class="h-3 w-3 text-green-600" />
              <Copy v-else class="h-3 w-3" />
            </button>
          </div>
        </div>
      </div>

      <div class="my-4 h-px bg-neutral-200" />

      <!-- Token 用量 -->
      <h3 class="mb-2.5 text-xs font-medium text-neutral-500">Token 用量</h3>
      <TokenUsagePanel :usage="session.tokenUsage" :duration-ms="durationMs" />

      <div class="my-4 h-px bg-neutral-200" />

      <!-- 时间信息 -->
      <h3 class="mb-2.5 text-xs font-medium text-neutral-500">时间信息</h3>
      <div class="space-y-1.5 text-[11px]">
        <div class="flex justify-between">
          <span class="text-neutral-400">创建时间</span>
          <span class="text-neutral-600">{{ formatDateTime(session.createdAt) }}</span>
        </div>
        <div class="flex justify-between">
          <span class="text-neutral-400">更新时间</span>
          <span class="text-neutral-600">{{ formatDateTime(session.updatedAt) }}</span>
        </div>
      </div>
    </div>
  </aside>
</template>
