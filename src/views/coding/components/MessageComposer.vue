<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { NSelect } from 'naive-ui'
import {
  Plus,
  ArrowUp,
  Square,
  Paperclip,
  X,
  BookOpen,
  Gauge,
} from 'lucide-vue-next'
import { MODELS, EXECUTION_MODES, useWorkspaceStore } from '../../../stores/workspace'
import { useSessionStore } from '../../../stores/session'
import { useConversation } from '../../../composables/useConversation'
import { formatTokens } from '../../../utils/time'
import type { AcpConfigOptionGroup, AcpConfigOptionValue } from '../../../types/acp'

const props = defineProps<{ sessionId: string }>()

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
const { send, stop, setModel } = useConversation()

const draft = ref('')
const attachments = ref<string[]>([])
const textareaEl = ref<HTMLTextAreaElement | null>(null)
const fileInputEl = ref<HTMLInputElement | null>(null)

const generating = computed(() => workspace.isGenerating(props.sessionId))
const canSend = computed(() => draft.value.trim().length > 0 && !generating.value)
const session = computed(() => sessionStore.get(props.sessionId))

/** 会话级模型下拉（design §3）：session/new configOptions 优先，
 * workspace.MODELS 静态数组兜底（浏览器 mock / 未建连时） */
const modelOptions = computed(() => {
  const config = session.value?.configOptions?.find(
    (o) => o.id === 'model' || o.category === 'model',
  )
  if (config?.options?.length) {
    const flat: { label: string; value: string }[] = []
    for (const opt of config.options) {
      if ('value' in opt) {
        const v = opt as AcpConfigOptionValue
        flat.push({ label: v.name ?? v.value, value: v.value })
      } else {
        const g = opt as AcpConfigOptionGroup
        for (const sub of g.options ?? []) {
          flat.push({ label: g.group ? `${g.group} / ${sub.name ?? sub.value}` : (sub.name ?? sub.value), value: sub.value })
        }
      }
    }
    if (flat.length) return flat
  }
  return MODELS.map((m) => ({ label: m, value: m }))
})

/** 下拉当前值：会话已建连用 session.model（ACP currentValue），否则用全局默认 */
const currentModel = computed({
  get: () => session.value?.model ?? workspace.selectedModel,
  set: (v: string) => {
    workspace.selectedModel = v
    if (session.value) session.value.model = v
    void setModel(props.sessionId, v)
  },
})

const modeOptions = EXECUTION_MODES.map((m) => ({ label: m.label, value: m.value }))

function autoResize() {
  const el = textareaEl.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = `${Math.min(el.scrollHeight, 200)}px`
}

watch(draft, () => nextTick(autoResize))

// 切换会话时清空草稿与附件，但不抢占焦点
watch(
  () => props.sessionId,
  () => {
    draft.value = ''
    attachments.value = []
    nextTick(autoResize)
  },
)

onMounted(() => {
  // 仅在页面初次进入且无其他焦点时自动聚焦
  const active = document.activeElement
  if (!active || active === document.body) textareaEl.value?.focus()
  autoResize()
})

function submit() {
  const text = draft.value.trim()
  if (!text || generating.value) return
  draft.value = ''
  attachments.value = []
  nextTick(autoResize)
  void send(props.sessionId, text)
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    submit()
  }
}

function pickFiles(e: Event) {
  const input = e.target as HTMLInputElement
  for (const f of input.files ?? []) attachments.value.push(f.name)
  input.value = ''
}
</script>

<template>
  <div class="shrink-0 px-6 pb-3">
    <div class="mx-auto max-w-3xl">
      <!-- 输入容器 -->
      <div
        class="rounded-xl border border-neutral-200 bg-white transition-colors focus-within:border-neutral-400"
      >
        <!-- 附件 chips -->
        <div v-if="attachments.length" class="flex flex-wrap gap-1.5 px-3 pt-2.5">
          <span
            v-for="(name, i) in attachments"
            :key="`${name}-${i}`"
            class="flex items-center gap-1 rounded-md border border-neutral-200 bg-neutral-50 px-2 py-0.5 text-xs text-neutral-600"
          >
            <Paperclip class="h-3 w-3 text-neutral-400" />
            <span class="max-w-40 truncate">{{ name }}</span>
            <button
              class="cursor-pointer text-neutral-400 hover:text-neutral-600"
              @click="attachments.splice(i, 1)"
            >
              <X class="h-3 w-3" />
            </button>
          </span>
        </div>

        <!-- 第一层：多行输入 -->
        <textarea
          ref="textareaEl"
          v-model="draft"
          rows="1"
          placeholder="发送消息，Enter 发送，Shift+Enter 换行"
          class="block max-h-[200px] w-full resize-none bg-transparent px-4 pt-3 pb-1 text-[13.5px] leading-6 text-neutral-800 outline-none placeholder:text-neutral-400"
          @keydown="onKeydown"
        />

        <!-- 第二层：操作工具栏 -->
        <div class="flex items-center gap-1.5 px-2.5 pb-2">
          <input ref="fileInputEl" type="file" multiple class="hidden" @change="pickFiles" />
          <button
            class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-100"
            title="添加附件"
            @click="fileInputEl?.click()"
          >
            <Plus class="h-4 w-4" />
          </button>

          <NSelect
            v-model:value="currentModel"
            :options="modelOptions"
            size="small"
            :consistent-menu-width="false"
            :disabled="generating"
            class="!w-56"
          />
          <NSelect
            v-model:value="workspace.executionMode"
            :options="modeOptions"
            size="small"
            :consistent-menu-width="false"
            class="!w-24"
          />

          <div class="flex-1" />

          <button
            v-if="generating"
            class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-full border border-neutral-300 text-neutral-600 hover:bg-neutral-100"
            title="停止生成"
            @click="stop(sessionId)"
          >
            <Square class="h-3.5 w-3.5" />
          </button>
          <button
            v-else
            class="flex h-8 w-8 items-center justify-center rounded-full transition-colors"
            :class="canSend ? 'cursor-pointer bg-blue-500 text-white hover:bg-blue-600' : 'cursor-not-allowed bg-neutral-100 text-neutral-300'"
            :disabled="!canSend"
            title="发送"
            @click="submit"
          >
            <ArrowUp class="h-4 w-4" />
          </button>
        </div>
      </div>

      <!-- 输入框下方轻量状态 -->
      <div class="mt-1.5 flex items-center justify-between text-[11px] text-neutral-400">
        <span class="flex items-center gap-1">
          <BookOpen class="h-3 w-3" />
          聊天模式
          <span class="mx-1 text-neutral-300">·</span>
          <Gauge class="h-3 w-3" />
          {{ EXECUTION_MODES.find((m) => m.value === workspace.executionMode)?.label }}
        </span>
        <span v-if="session && session.tokenUsage.total > 0">
          Token {{ formatTokens(session.tokenUsage.total) }}
        </span>
      </div>
    </div>
  </div>
</template>
