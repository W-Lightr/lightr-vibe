<script setup lang="ts">
// 一轮 assistant 回复 —— 对齐 codeg 的 `CompletedTurnContent`。
//
// 结构：
//   ┌ 工作了 39 秒 ⌄ ─────────────────────  ← 标题行（带下划线）
//   │ [被折叠的过程内容]
//   ├ 正文 / 工具胶囊 / 正文 / ...            ← 正文与工具**交错**平铺
//   └ 统计栏（用时 · token · 复制 · 重新生成 · 分叉）
//
// 核心不变式：**尾部回答恒可见，不进折叠**。
// 「要不要显示折叠头」看 body 里有没有过程分区 —— 纯文本回复没有。
import { computed } from 'vue'
import ContentPartView from './ContentPartView.vue'
import TurnStatsBar from './TurnStatsBar.vue'
import {
  collectReplyArtifacts,
  collectText,
  groupAdjacentTools,
  splitTrailingAnswer,
} from '../../../utils/contentParts'
import { formatDuration } from '../../../utils/time'
import type { ContentPart } from '../../../types/contentPart'
import type { TokenUsage } from '../../../types/session'

const props = defineProps<{
  parts: ContentPart[]
  streaming: boolean
  current: boolean
  durationMs: number | null
  usage: TokenUsage | null
  completedAt: number | null
  /** 过程内容的折叠态（父级持有，保证 epoch 级联收起生效） */
  open: boolean
  /** 会话所属项目 —— 文件卡片点开时用它定位项目 */
  projectId: string
}>()

const emit = defineEmits<{
  'update:open': [boolean]
  regenerate: []
  fork: []
}>()

const split = computed(() => splitTrailingAnswer(props.parts))

/**
 * body 里是否真的有过程分区。
 *
 * 用这个判断要不要显示折叠头，**不要**用 `trailing` 是否为空 —— 纯文本回复的
 * trailing 也是空的（见 utils/contentParts.ts 的边界说明）。
 */
const hasProgress = computed(() =>
  split.value.body.some(
    (p) => p.type === 'reasoning' || p.type === 'tool-call' || p.type === 'tool-group',
  ),
)

/**
 * 是否可以折叠（把过程收起来）。
 *
 * 对齐 codeg 的 `foldable`：
 * `progress.length > 0 && (!completed || hasVisibleAnswer(answer))`
 *
 * 关键在 `!completed` 这一半 —— **运行中的回复永远可折叠，且默认展开**。
 * 收尾正文还没写出来，用它来判断「有没有回答」会把整段流式输出都藏掉。
 *
 * 反过来，如果一轮**已结束**且**没有最终回答**（整段以思考/工具结尾），
 * 折叠起来会只剩一个光秃秃的标题，这才是真的「内容全没了」—— 那种轮次
 * 平铺显示（`flatten = true`）。
 */
const canFold = computed(() => {
  if (!hasProgress.value) return false
  // 运行中：永远可折叠（正文还没写完，不能拿「没有回答」否掉）
  if (props.streaming) return true
  // 已结束：只有存在可见回答时才折叠，否则平铺，避免折叠成空壳
  return split.value.trailing.length > 0
})

/** 平铺：已结束且没有最终回答（折叠会变成空壳） */
const flatten = computed(() => hasProgress.value && !canFold.value)

/**
 * 是否显示顶部状态行。
 *
 * 只要 body 里有过程分区就显示 —— **包括运行中**。
 *
 * 之前是 `hasProgress && !flatten`，而运行中 trailing 为空 → flatten=true →
 * 状态行被隐藏，工具卡孤零零挂在左侧、没有上下文，显得很突兀。
 */
const showHeader = computed(() => hasProgress.value)

const copyText = computed(() => collectText(props.parts))

/**
 * 文件改动汇总作为一个**分区**追加到末尾。
 *
 * 这样它天然走 `ContentPartView` 的那条渲染分支，不必在模板里再写一份卡片；
 * 同时它属于「回答类」的展示位置 —— 放在统计栏之前、回答之后。
 */
const artifactsPart = computed<ContentPart | null>(() => {
  const groups = collectReplyArtifacts(props.parts)
  if (!groups) return null
  return { type: 'reply-artifacts', added: groups.added, changed: groups.changed }
})

/** 生成中不显示文件卡片（列表还在增长，会一直跳） */
const showArtifacts = computed(() => !props.streaming)

/** 折叠区里的过程内容（相邻工具合并成胶囊） */
const bodyParts = computed(() => groupAdjacentTools(split.value.body))

/** 平铺分支：整轮内容 + 文件卡片 */
const flatPartsWithArtifacts = computed<ContentPart[]>(() => {
  const base = groupAdjacentTools(props.parts)
  return artifactsPart.value && showArtifacts.value ? [...base, artifactsPart.value] : base
})

/** 回答分支：尾部回答 + 文件卡片 */
const answerPartsWithArtifacts = computed<ContentPart[]>(() => {
  const base = groupAdjacentTools(split.value.trailing)
  return artifactsPart.value && showArtifacts.value ? [...base, artifactsPart.value] : base
})

/**
 * 分区渲染用的稳定 key。
 *
 * **不能用数组下标**：流式结束时 `groupAdjacentTools` 会把相邻工具从「多个
 * tool-call」换成「一个 tool-group」，下标重排会让 Vue 复用错组件实例 ——
 * 用户刚展开的工具卡会突然变成另一条的内容。这里用分区自身的内容标识。
 *
 * 不同分区列表里可能撞 key（body / trailing 来自同一段 parts），所以调用方
 * 还要拼上所在区的前缀。
 */
function partKey(part: ContentPart, i: number): string {
  switch (part.type) {
    case 'tool-call':
      return `tool:${part.id}`
    case 'tool-group':
      return `group:${part.items.map((t) => t.id).join(',')}`
    case 'reply-artifacts':
      return 'artifacts'
    case 'turn-error':
    case 'reasoning':
    case 'text':
    case 'ask-result':
      // 文本类没有稳定 id，下标够用 —— 它们的身份就是「第几段」，且不会因
      // 合组而重排
      return `${part.type}:${i}`
  }
}


/** 标题行：`工作了 39 秒`（运行中显示「正在工作」） */
/** 正在执行的那条工具（取 body 里最后一条 running 的） */
const activeTool = computed(() => {
  if (!props.streaming) return null
  const all = [...split.value.body, ...split.value.trailing]
  for (let i = all.length - 1; i >= 0; i -= 1) {
    const p = all[i]
    if (p.type === 'tool-call' && p.status === 'running') return p
    if (p.type === 'tool-group') {
      for (let j = p.items.length - 1; j >= 0; j -= 1) {
        if (p.items[j].status === 'running') return p.items[j]
      }
    }
  }
  return null
})

/** 运行中工具的动词短语（比裸「运行中」信息量高） */
const RUNNING_LABEL: Record<string, string> = {
  execute: '正在运行命令',
  edit: '正在编辑文件',
  delete: '正在删除文件',
  move: '正在移动文件',
  read: '正在读取文件',
  search: '正在搜索',
  fetch: '正在抓取网页',
  think: '正在工作',
}

/**
 * 状态行文案。
 *
 * 运行中优先显示**当前正在做什么**（「正在运行命令」），而不是笼统的
 * 「正在工作」—— 用户看进度行时最想知道的就是这个。
 */
const workedLabel = computed(() => {
  if (props.streaming) {
    const tool = activeTool.value
    if (tool?.kind && RUNNING_LABEL[tool.kind]) return RUNNING_LABEL[tool.kind]
    if (tool) return '正在执行'
    return '正在工作'
  }
  if (props.durationMs === null) return '工作已完成'
  return `工作了 ${formatDuration(props.durationMs)}`
})

function toggle() {
  emit('update:open', !props.open)
}
</script>

<template>
  <div class="mb-6 space-y-3">
    <!--
      折叠头：`工作了 39 秒 ⌄` + 下划线（codeg 的 HEADER_CLASS 同款）。
      只有 body 里真有过程内容时才出现 —— 纯文本回复不该有个空折叠头。
    -->
    <button
      v-if="showHeader"
      class="flex w-full cursor-pointer items-center gap-1 border-b border-neutral-200 pb-1.5 text-xs font-medium text-neutral-400 transition-colors hover:text-neutral-600"
      :title="open ? '收起过程' : '展开过程'"
      @click="toggle"
    >
      <!--
        运行中给文字加扫光（codeg 的 Shimmer）。
        底色是**黑字**（neutral-800），扫光用中性灰高亮掠过 —— 上一版用的是
        蓝色渐变，在状态行这种小字号里太抢眼，已按反馈改为黑字。
      -->
      <span :class="streaming ? 'shimmer-dark' : 'text-neutral-500'">{{ workedLabel }}</span>
      <span class="flex-1" />
      <svg
        class="h-3.5 w-3.5 shrink-0 opacity-50 transition-transform"
        :class="open && 'rotate-90'"
        viewBox="0 0 16 16"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        aria-hidden="true"
      >
        <path d="M6 3.5 10.5 8 6 12.5" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>

    <!-- 退化平铺：trailing 为空时直接显示整轮内容，不折叠 -->
    <template v-if="flatten">
      <ContentPartView
        v-for="(part, i) in flatPartsWithArtifacts"
        :key="`flat:${partKey(part, i)}`"
        :part="part"
        :project-id="projectId"
        :allow-retry="current"
        @retry="emit('regenerate')"
      />
    </template>

    <!-- 被折叠的过程内容 -->
    <div v-else-if="canFold && open" class="space-y-3">
      <ContentPartView
        v-for="(part, i) in bodyParts"
        :key="`body:${partKey(part, i)}`"
        :part="part"
        :project-id="projectId"
      />
    </div>

    <!-- 尾部回答：恒可见，不进折叠 -->
    <ContentPartView
      v-for="(part, i) in answerPartsWithArtifacts"
      :key="`tail:${partKey(part, i)}`"
      :part="part"
      :project-id="projectId"
      :allow-retry="current"
      @retry="emit('regenerate')"
    />

    <!-- 统计栏（生成中不显示，数据还在变） -->
    <TurnStatsBar
      :duration-ms="durationMs"
      :usage="usage"
      :copy-text="copyText"
      :current="current"
      :completed-at="completedAt"
      :running="streaming"
      @regenerate="emit('regenerate')"
      @fork="emit('fork')"
    />
  </div>
</template>

<style scoped>


/*
 * 黑字扫光（运行中的状态下标题）。
 * background-clip: text + 渐变高亮从右向左掠过；底色保持深灰，不偏色。
 */
.shimmer-dark {
  --shimmer-base: #404040;
  --shimmer-hi: #a3a3a3;
  background-image: linear-gradient(
    90deg,
    var(--shimmer-base) 0%,
    var(--shimmer-base) 35%,
    var(--shimmer-hi) 50%,
    var(--shimmer-base) 65%,
    var(--shimmer-base) 100%
  );
  background-size: 250% 100%;
  background-repeat: no-repeat;
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
  animation: shimmer-dark-sweep 2s linear infinite;
}

@keyframes shimmer-dark-sweep {
  from {
    background-position: 100% center;
  }
  to {
    background-position: 0% center;
  }
}

@media (prefers-reduced-motion: reduce) {
  .shimmer-dark {
    animation: none;
    color: var(--shimmer-base);
    -webkit-text-fill-color: currentColor;
  }
}

</style>
