<script setup lang="ts">
// 单个工具调用卡片 —— 对齐 codeg 的 ToolCallPart。
//
// 版式（参考设计图）：
//   [>_] $ command ...                    (✓ 已完成)  ⌄
//
// - 左侧是工具图标（按 ACP `kind`）
// - 中间是标题 / 命令摘要（等宽，过长省略号截断）
// - 右侧是**状态胶囊**：已完成（绿）/ 运行中（蓝）/ 失败（红）
// - 最右是展开箭头
//
// 图标用 `kind` 而不是猜中文标题 —— 按标题关键词分类在上一轮迭代已经踩过坑。
import { computed, ref } from 'vue'
import {
  Check,
  ChevronDown,
  ChevronRight,
  FilePen,
  FileText,
  LoaderCircle,
  Search,
  SquareTerminal,
  Wrench,
  X,
} from 'lucide-vue-next'
import type { ToolCallPartData } from '../../../types/contentPart'

const props = defineProps<{ part: ToolCallPartData }>()

const expanded = ref(false)

/**
 * `kind` → 图标。
 *
 * `delete` / `move` 归到编辑类：对用户来说都是「改了文件」，没必要为它们
 * 再单独准备图标。
 */
const KIND_ICON = {
  execute: SquareTerminal,
  edit: FilePen,
  delete: FilePen,
  move: FilePen,
  read: FileText,
  search: Search,
  fetch: Search,
  switch_mode: Wrench,
  think: Wrench,
  other: Wrench,
} as const

const icon = computed(() => {
  const kind = props.part.kind ?? 'other'
  return KIND_ICON[kind as keyof typeof KIND_ICON] ?? Wrench
})

/** 标题：优先 agent 给的 name，缺失时按 kind 兜底 */
const title = computed(() => {
  if (props.part.name.trim()) return props.part.name
  switch (props.part.kind) {
    case 'execute':
      return '运行了命令'
    case 'edit':
      return '编辑了文件'
    case 'read':
      return '读取了文件'
    case 'search':
      return '搜索了内容'
    default:
      return '工具调用'
  }
})

/**
 * 一行摘要：命令文本或文件路径。
 *
 * 命令内容统一压成一行（换行会破坏单行布局），过长由容器省略号截断 ——
 * 不在数据层截断，那样会丢掉用户展开后想看的信息。
 */
const summary = computed(() => {
  const raw = (props.part.input || props.part.paths?.[0] || '').trim()
  return raw.replace(/\s+/g, ' ')
})

const hasDetail = computed(() => Boolean(props.part.input || props.part.result))

/** 状态胶囊：codeg 用「✓ 已完成」这种文字胶囊，而不是裸图标 */
const statusLabel = computed(() => {
  switch (props.part.status) {
    case 'running':
      return '运行中'
    case 'error':
      return '失败'
    default:
      return '已完成'
  }
})
</script>

<template>
  <div class="max-w-full overflow-hidden rounded-lg border border-neutral-200 bg-white">
    <button
      class="flex w-full min-w-0 items-center gap-2 px-2.5 py-2 text-left text-xs transition-colors hover:bg-neutral-50"
      :class="hasDetail ? 'cursor-pointer' : 'cursor-default'"
      :disabled="!hasDetail"
      @click="expanded = !expanded"
    >
      <!-- 工具图标 -->
      <component :is="icon" class="h-3.5 w-3.5 shrink-0 text-neutral-400" />

      <!-- 标题 + 摘要。命令类只显示 `$ xxx`，标题已在胶囊里聚合过，不必重复 -->
      <span
        v-if="summary"
        class="min-w-0 flex-1 truncate font-mono text-[11px] text-neutral-600"
        :title="summary"
      >
        <span v-if="part.kind === 'execute'" class="text-neutral-400">$ </span>{{ summary }}
      </span>
      <span v-else class="min-w-0 flex-1 truncate text-neutral-600">{{ title }}</span>

      <!-- 状态胶囊 -->
      <span
        class="flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium"
        :class="{
          'bg-emerald-50 text-emerald-600': part.status === 'done',
          'bg-red-50 text-red-600': part.status === 'error',
          'bg-blue-50 text-blue-600': part.status === 'running',
        }"
      >
        <LoaderCircle v-if="part.status === 'running'" class="h-3 w-3 animate-spin" />
        <X v-else-if="part.status === 'error'" class="h-3 w-3" />
        <Check v-else class="h-3 w-3" />
        {{ statusLabel }}
      </span>

      <ChevronDown v-if="expanded" class="h-3.5 w-3.5 shrink-0 text-neutral-300" />
      <ChevronRight v-else-if="hasDetail" class="h-3.5 w-3.5 shrink-0 text-neutral-300" />
    </button>

    <div v-if="expanded && hasDetail" class="border-t border-neutral-100 px-2.5 py-2">
      <template v-if="part.input">
        <p class="text-[10px] font-medium text-neutral-400">参数</p>
        <pre
          class="mt-0.5 max-h-48 overflow-auto whitespace-pre-wrap break-all rounded bg-neutral-50 px-2 py-1.5 font-mono text-[11px] leading-5 text-neutral-600"
        >{{ part.input }}</pre>
      </template>

      <template v-if="part.result">
        <p class="mt-2 text-[10px] font-medium text-neutral-400">输出</p>
        <pre
          class="mt-0.5 max-h-64 overflow-auto whitespace-pre-wrap break-all rounded bg-neutral-50 px-2 py-1.5 font-mono text-[11px] leading-5 text-neutral-600"
        >{{ part.result }}</pre>
      </template>
    </div>
  </div>
</template>
