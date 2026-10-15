<script setup lang="ts">
// elicitation 提问卡（Questions 型）：渲染 prompt + 单选/多选/文本 + 提交/跳过。
// 独立卡片，浮在 composer 输入框上方（与权限卡同一区域/层级，见 MessageComposer）。
// 提交后本地标记已回答、按钮禁用；提交/跳过均调用 useAcpQuestion（服务层唯一 invoke 点）。
import { computed, ref } from 'vue'
import { HelpCircle, Check, X } from 'lucide-vue-next'
import type { AcpQuestionEvent } from '../../../types/acp'
import { useAcpQuestion } from '../../../composables/useAcpQuestion'

const props = defineProps<{ event: AcpQuestionEvent }>()

const { submit, skip } = useAcpQuestion()

/** 本期一次一问（design D3）：取首题渲染，其余忽略 */
const question = computed(() => props.event.questions[0] ?? null)

/** 单选：选中值；多选：选中值集合；文本：自由输入 */
const singleValue = ref<string | null>(null)
const multiValues = ref<string[]>([])
const textValue = ref('')

const answered = ref(false)

function toggleMulti(value: string) {
  const i = multiValues.value.indexOf(value)
  if (i >= 0) multiValues.value.splice(i, 1)
  else multiValues.value.push(value)
}

/** 当前答案（按题型收集）；未作答时返回空 */
const currentValues = computed<string[]>(() => {
  const q = question.value
  if (!q) return []
  if (q.kind === 'single') return singleValue.value ? [singleValue.value] : []
  if (q.kind === 'multi') return [...multiValues.value]
  return textValue.value.trim() ? [textValue.value.trim()] : []
})

/** 是否已填答（提交按钮可用判据；跳过始终可用） */
const canSubmit = computed(() => currentValues.value.length > 0)

const busy = ref(false)

async function onSubmit() {
  if (busy.value || !canSubmit.value) return
  busy.value = true
  try {
    const q = question.value
    const answers = q ? [{ fieldId: q.id, values: currentValues.value }] : []
    answered.value = true
    await submit(props.event, answers)
  } finally {
    busy.value = false
  }
}

async function onSkip() {
  if (busy.value) return
  busy.value = true
  try {
    answered.value = true
    await skip(props.event)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="px-3 py-2">
    <!-- 头部：图标 + 「AI 提问」 -->
    <div class="flex items-center gap-2">
      <HelpCircle class="h-4 w-4 shrink-0 text-blue-500" />
      <span class="shrink-0 text-[13px] font-medium text-neutral-800">AI 提问</span>
    </div>

    <!-- 问题 prompt -->
    <p v-if="question" class="mt-1 pl-6 text-[12.5px] leading-snug text-neutral-700">
      {{ question.prompt || question.id }}
    </p>

    <!-- 选项 / 文本输入（answered 后只读） -->
    <div v-if="question" class="mt-1.5 space-y-1.5 pl-6">
      <!-- 单选：每个选项是长方形圆角框 -->
      <template v-if="question.kind === 'single'">
        <label
          v-for="opt in question.options"
          :key="opt.value"
          class="flex cursor-pointer items-center gap-2 rounded-lg border px-2.5 py-1.5 text-[12.5px] transition-colors"
          :class="
            singleValue === opt.value
              ? 'border-blue-400 bg-blue-50/60 text-blue-700'
              : 'border-neutral-200 bg-white text-neutral-600 hover:border-neutral-300 hover:bg-neutral-50'
          "
          :style="answered ? 'cursor: default; opacity: 0.6' : ''"
        >
          <input
            type="radio"
            name="acp-question-single"
            class="h-3.5 w-3.5 shrink-0"
            :value="opt.value"
            :checked="singleValue === opt.value"
            :disabled="answered"
            @change="singleValue = opt.value"
          />
          <span class="min-w-0 flex-1 truncate">{{ opt.label }}</span>
        </label>
      </template>

      <!-- 多选：每个选项是长方形圆角框 -->
      <template v-else-if="question.kind === 'multi'">
        <label
          v-for="opt in question.options"
          :key="opt.value"
          class="flex cursor-pointer items-center gap-2 rounded-lg border px-2.5 py-1.5 text-[12.5px] transition-colors"
          :class="
            multiValues.includes(opt.value)
              ? 'border-blue-400 bg-blue-50/60 text-blue-700'
              : 'border-neutral-200 bg-white text-neutral-600 hover:border-neutral-300 hover:bg-neutral-50'
          "
          :style="answered ? 'cursor: default; opacity: 0.6' : ''"
        >
          <input
            type="checkbox"
            class="h-3.5 w-3.5 shrink-0"
            :value="opt.value"
            :checked="multiValues.includes(opt.value)"
            :disabled="answered"
            @change="toggleMulti(opt.value)"
          />
          <span class="min-w-0 flex-1 truncate">{{ opt.label }}</span>
        </label>
      </template>

      <!-- 自由文本 -->
      <template v-else>
        <input
          v-model="textValue"
          type="text"
          class="w-full rounded-md border border-neutral-200 bg-neutral-50 px-2.5 py-1.5 text-[12.5px] text-neutral-700 outline-none placeholder:text-neutral-400 focus:border-neutral-300 disabled:opacity-60"
          placeholder="请输入回答"
          :disabled="answered"
          @keydown.enter.prevent="onSubmit"
        />
      </template>
    </div>

    <!-- 按钮：右对齐；提交后标记已回答并禁用 -->
    <div class="mt-2 flex items-center justify-end gap-2">
      <span v-if="answered" class="mr-auto flex items-center gap-1 text-[11px] text-green-600">
        <Check class="h-3.5 w-3.5" />
        已回答
      </span>
      <button
        class="flex h-7 cursor-pointer items-center gap-1 rounded-lg border border-neutral-300 bg-white px-2.5 text-xs text-neutral-600 hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-50"
        :disabled="busy || answered"
        @click="onSkip"
      >
        <X class="h-3.5 w-3.5" />
        跳过
      </button>
      <button
        class="flex h-7 cursor-pointer items-center gap-1 rounded-lg bg-neutral-900 px-2.5 text-xs text-white hover:bg-neutral-700 disabled:cursor-not-allowed disabled:opacity-50"
        :disabled="busy || answered || !canSubmit"
        @click="onSubmit"
      >
        <Check class="h-3.5 w-3.5" />
        提交
      </button>
    </div>
  </div>
</template>
