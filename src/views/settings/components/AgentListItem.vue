<script setup lang="ts">
// Agent 列表行：拖拽把手（视觉占位）+ 图标 + 名称 + 状态点 + 检测徽章 + 刷新。
// 禁用行（即将推出的 Agent）不可选中，点击给中文占位反馈（PRD R2）。
import { LoaderCircle, RefreshCw, XCircle, CheckCircle2, CircleSlash, GripVertical } from 'lucide-vue-next'
import type { AgentDef, AgentStatus } from '../../../types/agent'

const props = defineProps<{
  agent: AgentDef
  status: AgentStatus
  selected: boolean
}>()

const emit = defineEmits<{
  select: []
  refresh: []
}>()

/** 徽章文案与配色（ui-style.md：绿 PASS / 红 FAIL / 灰未安装 / spin 检测中） */
function badgeClass(status: AgentStatus): string {
  switch (status) {
    case 'pass':
      return 'bg-green-50 text-green-600'
    case 'fail':
      return 'bg-red-50 text-red-600'
    case 'detecting':
      return 'bg-neutral-100 text-neutral-500'
    case 'not-installed':
      return 'bg-neutral-100 text-neutral-400'
    case 'unknown':
      return 'bg-neutral-100 text-neutral-400'
  }
}

function badgeText(status: AgentStatus): string {
  switch (status) {
    case 'pass':
      return 'PASS'
    case 'fail':
      return 'FAIL'
    case 'detecting':
      return '检测中'
    case 'not-installed':
      return '未安装'
    case 'unknown':
      return '未检测'
  }
}

/** 状态点颜色：pass 蓝（ui-style.md 强调色）/ fail 红 / 其余灰 */
function dotClass(status: AgentStatus): string {
  switch (status) {
    case 'pass':
      return 'bg-blue-500'
    case 'fail':
      return 'bg-red-500'
    case 'detecting':
      return 'bg-blue-500 animate-pulse'
    case 'not-installed':
    case 'unknown':
      return 'bg-neutral-300'
  }
}
</script>

<template>
  <div
    class="flex cursor-pointer items-center gap-1.5 rounded-md border px-2 py-1.5 transition-colors"
    :class="
      props.selected
        ? 'border-neutral-300 bg-neutral-200/70'
        : 'border-transparent hover:bg-neutral-100'
    "
    @click="emit('select')"
  >
    <!-- 拖拽把手：视觉占位，本期不可拖（PRD R2） -->
    <GripVertical class="h-3.5 w-3.5 shrink-0 text-neutral-300" />

    <!-- lobehub 内联 SVG（currentColor，1em） -->
    <span class="flex h-5 w-5 shrink-0 items-center justify-center text-neutral-700">
      <span class="h-4 w-4 [&>svg]:h-4 [&>svg]:w-4" v-html="props.agent.iconSvg" />
    </span>

    <span class="min-w-0 flex-1 truncate text-[13px] text-neutral-700">{{ props.agent.name }}</span>

    <!-- 状态点（禁用占位行不显示检测徽章/刷新） -->
    <span class="h-1.5 w-1.5 shrink-0 rounded-full" :class="dotClass(props.status)" />

    <!-- 检测徽章 -->
    <span
      v-if="props.agent.enabled"
      class="flex shrink-0 items-center gap-0.5 rounded-md px-1.5 py-0.5 text-[10px] font-medium"
      :class="badgeClass(props.status)"
    >
      <LoaderCircle v-if="props.status === 'detecting'" class="h-3 w-3 animate-spin" />
      <CheckCircle2 v-else-if="props.status === 'pass'" class="h-3 w-3" />
      <XCircle v-else-if="props.status === 'fail'" class="h-3 w-3" />
      <CircleSlash v-else class="h-3 w-3" />
      {{ badgeText(props.status) }}
    </span>
    <span v-else class="shrink-0 rounded-md bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-400">
      即将推出
    </span>

    <!-- 刷新按钮：重新执行检测（仅可配置 Agent） -->
    <button
      v-if="props.agent.enabled"
      class="flex h-6 w-6 shrink-0 cursor-pointer items-center justify-center rounded-md text-neutral-400 hover:bg-neutral-200/70 hover:text-neutral-600"
      title="重新检测"
      @click.stop="emit('refresh')"
    >
      <RefreshCw class="h-3.5 w-3.5" :class="props.status === 'detecting' && 'animate-spin'" />
    </button>
  </div>
</template>
