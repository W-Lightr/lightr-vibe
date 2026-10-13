<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import {
  ChevronRight,
  Brain,
  Copy,
  Check,
  RefreshCw,
  GitBranch,
  AlertCircle,
  Square,
  Settings,
} from 'lucide-vue-next'
import type { Message } from '../../../types/session'
import MarkdownContent from './MarkdownContent.vue'
import ToolCallPanel from './ToolCallPanel.vue'
import { useConversation } from '../../../composables/useConversation'
import { useWorkspaceStore } from '../../../stores/workspace'

const props = defineProps<{ message: Message }>()

const workspace = useWorkspaceStore()
const router = useRouter()
const { regenerate, stop, fork } = useConversation()

const copied = ref(false)
const isUser = computed(() => props.message.role === 'user')
const isThinking = computed(() => props.message.type === 'thinking')
const isError = computed(() => props.message.type === 'error')
const isStreaming = computed(() => props.message.status === 'streaming')
const generating = computed(() => workspace.isGenerating(props.message.sessionId))

// 思考块折叠态：默认折叠（含流式中），用户点击头部手动展开查看推理内容。
const thinkingExpanded = ref(false)

/** 错误卡「去设置」按钮：dead（进程/未安装）与 auth（凭据）两类错误引导跳设置页（PRD R4） */
const showSettingsLink = computed(
  () => props.message.errorKind === 'dead' || props.message.errorKind === 'auth',
)

async function copy() {
  try {
    await navigator.clipboard.writeText(props.message.content)
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {
    /* 剪贴板不可用时静默忽略 */
  }
}

/** 从此分叉：复制源会话截至本条助手回复（含）的历史到新会话并打开 */
function onFork() {
  const forked = fork(props.message.sessionId, props.message.id)
  if (forked) workspace.openSession(forked.id)
}
</script>

<template>
  <!-- 用户消息：右对齐浅灰气泡 -->
  <div v-if="isUser" class="mb-5 flex justify-end">
    <div
      class="max-w-[75%] whitespace-pre-wrap break-words rounded-2xl rounded-br-md bg-neutral-100 px-4 py-2.5 text-[13.5px] leading-6 text-neutral-800"
    >
      {{ message.content }}
    </div>
  </div>

  <!-- 思考：紧凑头部行（图标 + 状态标签），点击展开轻量正文 -->
  <div v-else-if="isThinking" class="mb-3">
    <button
      class="flex cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-0.5 text-xs text-neutral-500 hover:bg-neutral-100 hover:text-neutral-700"
      @click="thinkingExpanded = !thinkingExpanded"
    >
      <ChevronRight class="h-3.5 w-3.5 transition-transform" :class="thinkingExpanded && 'rotate-90'" />
      <Brain class="h-3.5 w-3.5" />
      <span>{{ isStreaming ? '思考中…' : '思考完毕' }}</span>
    </button>
    <div
      v-if="thinkingExpanded"
      class="mt-1 ml-5 whitespace-pre-wrap break-words border-l-2 border-neutral-200 pl-3 text-xs leading-5 text-neutral-500"
    >
      {{ message.content }}
    </div>
  </div>

  <!-- 错误提示（PRD R4：dead / 凭据 / timeout 错误卡 + 「去设置」按钮） -->
  <div
    v-else-if="isError"
    class="mb-4 flex items-start gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-[13px] text-red-600"
  >
    <AlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
    <span class="min-w-0 flex-1 break-words">{{ message.content || '生成失败' }}</span>
    <button
      v-if="showSettingsLink"
      class="flex h-6 shrink-0 cursor-pointer items-center gap-1 rounded-md border border-red-200 bg-white px-2 text-xs text-red-600 hover:bg-red-100"
      @click="router.push('/settings')"
    >
      <Settings class="h-3 w-3" />
      去设置
    </button>
  </div>

  <!-- AI 回复 / 工具调用记录 -->
  <div v-else class="mb-5">
    <ToolCallPanel v-if="message.toolCalls?.length" :tool-calls="message.toolCalls" class="mb-2.5" />

    <MarkdownContent v-if="message.content" :content="message.content" />
    <span v-else-if="isStreaming" class="inline-block h-4 w-1.5 animate-pulse rounded-sm bg-neutral-400" />
    <span v-else-if="message.status === 'stopped'" class="text-xs text-neutral-400">已停止生成</span>

    <!-- 操作工具栏：仅完成的助手文本回复显示 -->
    <div v-if="message.type === 'text' && message.content && !isStreaming" class="mt-1.5 flex items-center gap-0.5">
      <button
        class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-md text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
        title="复制"
        @click="copy"
      >
        <Check v-if="copied" class="h-3.5 w-3.5 text-green-600" />
        <Copy v-else class="h-3.5 w-3.5" />
      </button>
      <button
        class="flex h-7 cursor-pointer items-center gap-1 rounded-md px-1.5 text-xs text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
        title="重新生成"
        :disabled="generating"
        :class="generating && 'cursor-not-allowed opacity-40'"
        @click="regenerate(message.sessionId, message.id)"
      >
        <RefreshCw class="h-3.5 w-3.5" />
        重新生成
      </button>
      <button
        class="flex h-7 cursor-pointer items-center gap-1 rounded-md px-1.5 text-xs text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
        title="从此分叉"
        @click="onFork"
      >
        <GitBranch class="h-3.5 w-3.5" />
        从此分叉
      </button>
      <button
        v-if="generating"
        class="flex h-7 cursor-pointer items-center gap-1 rounded-md px-1.5 text-xs text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
        title="停止生成"
        @click="stop(message.sessionId)"
      >
        <Square class="h-3 w-3" />
        停止
      </button>
    </div>
  </div>
</template>
