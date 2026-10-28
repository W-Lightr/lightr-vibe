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
 * 是否退化平铺（不做折叠）。
 *
 * 触发条件：`trailing` 为空 —— 整段都以思考/工具结尾，没有「最终回答」。
 * 这时如果还按折叠渲染，界面上只剩一个「工作了 X 秒」，正文内容全被藏在
 * 折叠块里，看起来像「内容全没了」（真实会话里大量出现）。
 *
 * 所以这种轮次直接平铺全部内容：宁可长一点，也不能让人以为回复是空的。
 */
const flatten = computed(() => split.value.trailing.length === 0)

/** 真正可折叠时才显示折叠头 */
const canCollapse = computed(() => hasProgress.value && !flatten.value)

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
      // 文本类没有稳定 id，下标够用 —— 它们的身份就是「第几段」，且不会因
      // 合组而重排
      return `${part.type}:${i}`
  }
}


/** 标题行：`工作了 39 秒`（运行中显示「正在工作」） */
const workedLabel = computed(() => {
  if (props.streaming) return '正在工作'
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
      v-if="canCollapse"
      class="flex w-full cursor-pointer items-center gap-1 border-b border-neutral-200 pb-1.5 text-xs font-medium text-neutral-400 transition-colors hover:text-neutral-600"
      :title="open ? '收起过程' : '展开过程'"
      @click="toggle"
    >
      <span :class="streaming && 'worked-shimmer'">{{ workedLabel }}</span>
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
    <div v-else-if="canCollapse && open" class="space-y-3">
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
/* 运行中标题的扫光（codeg 用 Shimmer 组件，这里用 CSS 达到同样效果） */
.worked-shimmer {
  background-image: linear-gradient(
    90deg,
    currentColor 0%,
    currentColor 35%,
    #3b82f6 50%,
    currentColor 65%,
    currentColor 100%
  );
  background-size: 250% 100%;
  background-repeat: no-repeat;
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
  animation: worked-sweep 1.4s linear infinite;
}

@keyframes worked-sweep {
  from {
    background-position: 100% center;
  }
  to {
    background-position: 0% center;
  }
}

@media (prefers-reduced-motion: reduce) {
  .worked-shimmer {
    animation: none;
    color: inherit;
    -webkit-text-fill-color: currentColor;
  }
}
</style>
