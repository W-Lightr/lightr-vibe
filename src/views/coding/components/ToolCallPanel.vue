<script setup lang="ts">
// 工具调用列表：类 codex 的展示风格。
//
// 每条工具占一行（☑ 运行了命令 / ✎ 编辑了文件），状态用前缀图标表达；
// 同类**连续**调用仍收成可折叠小节，避免几十条平铺。
//
// 分类优先用 ACP 的 `kind`（结构化、可靠），老消息没有 kind 时回退到
// 从 `name` 推断关键词。
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
import type { ToolCall } from '../../../types/session'

const props = defineProps<{
  toolCalls: ToolCall[]
  /** 所属轮次是否仍在生成 */
  running?: boolean
}>()

type ToolGroupKey = 'command' | 'edit' | 'read' | 'search' | 'other'

interface ToolGroup {
  key: ToolGroupKey
  label: string
  items: ToolCall[]
}

/**
 * `kind` → 类别。ACP 标准值有 read/edit/delete/move/search/execute/think/
 * fetch/switch_mode/other，这里归成 5 组。
 */
const KIND_MAP: Record<string, ToolGroupKey> = {
  execute: 'command',
  edit: 'edit',
  delete: 'edit',
  move: 'edit',
  read: 'read',
  search: 'search',
  fetch: 'search',
}

/**
 * name → 类别（仅在 `kind` 缺失时使用）。
 *
 * 只看 `name` 不看 `input`：input 是命令文本或路径，出现 read/edit 会被误判。
 * `command` 排最后，因为它最容易被「运行了 read xxx」这类措辞带偏。
 */
function classifyByName(name: string): ToolGroupKey {
  const t = name.toLowerCase()
  if (/(读取|已读|查看|read|cat|open)/.test(t)) return 'read'
  if (/(编辑|修改|写入|write|edit|patch|apply)/.test(t)) return 'edit'
  if (/(搜索|查找|grep|search|find|glob)/.test(t)) return 'search'
  if (/(运行|执行|命令|command|bash|shell|terminal|exec)/.test(t)) return 'command'
  return 'other'
}

function classify(tc: ToolCall): ToolGroupKey {
  if (tc.kind && KIND_MAP[tc.kind]) return KIND_MAP[tc.kind]
  return classifyByName(tc.name)
}

const GROUP_LABEL: Record<ToolGroupKey, string> = {
  command: '运行了命令',
  edit: '编辑了文件',
  read: '读取了文件',
  search: '搜索了内容',
  other: '其他操作',
}

const GROUP_ICON = {
  command: SquareTerminal,
  edit: FilePen,
  read: FileText,
  search: Search,
  other: Wrench,
} as const

/** 按类别分组，保持首次出现的顺序（与对话时序一致） */
const groups = computed<ToolGroup[]>(() => {
  const map = new Map<ToolGroupKey, ToolCall[]>()
  for (const tc of props.toolCalls) {
    const key = classify(tc)
    const list = map.get(key)
    if (list) list.push(tc)
    else map.set(key, [tc])
  }
  return [...map.entries()].map(([key, items]) => ({
    key,
    label: GROUP_LABEL[key],
    items,
  }))
})

/** 单条工具的展开态（点开看 result） */
const expandedGroups = ref<Record<string, boolean>>({})

function isExpanded(g: ToolGroup): boolean {
  const explicit = expandedGroups.value[g.key]
  if (explicit !== undefined) return explicit
  // 未手动切换：运行中自动展开「含在跑条目」的分组
  return props.running === true && g.items.some((i) => i.status === 'running')
}

function toggle(g: ToolGroup) {
  expandedGroups.value = { ...expandedGroups.value, [g.key]: !isExpanded(g) }
}

function groupStatus(g: ToolGroup): 'running' | 'error' | 'done' {
  if (g.items.some((i) => i.status === 'running')) return 'running'
  if (g.items.some((i) => i.status === 'error')) return 'error'
  return 'done'
}

/** 条目摘要（命令文本 / 文件路径），过长截断 */
function itemSummary(tc: ToolCall): string {
  const raw = (tc.input || tc.paths?.[0] || '').trim()
  return raw.replace(/\s+/g, ' ').slice(0, 240)
}
</script>

<template>
  <div class="flex flex-col gap-0.5">
    <div v-for="g in groups" :key="g.key">
      <!-- 分组头：☑ 运行了命令 · 2 个 -->
      <button
        class="group flex w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-1 text-xs text-neutral-600 transition-colors hover:bg-neutral-100 hover:text-neutral-800"
        @click="toggle(g)"
      >
        <component
          :is="GROUP_ICON[g.key]"
          class="h-3.5 w-3.5 shrink-0 text-neutral-400"
        />
        <span class="shrink-0">{{ g.label }}</span>
        <span class="text-neutral-400">· {{ g.items.length }} 个</span>

        <span class="flex-1" />

        <!-- 状态前缀。等宽、与文字同基线，模仿 codex 的「☑」位置 -->
        <span class="flex h-3.5 w-3.5 shrink-0 items-center justify-center">
          <LoaderCircle
            v-if="groupStatus(g) === 'running'"
            class="h-3 w-3 animate-spin text-blue-500"
          />
          <X v-else-if="groupStatus(g) === 'error'" class="h-3 w-3 text-red-500" />
          <Check v-else class="h-3 w-3 text-emerald-500" />
        </span>
        <ChevronDown v-if="isExpanded(g)" class="h-3.5 w-3.5 shrink-0 text-neutral-300" />
        <ChevronRight v-else class="h-3.5 w-3.5 shrink-0 text-neutral-300" />
      </button>

      <!-- 分组内容 -->
      <div
        v-if="isExpanded(g)"
        class="mt-0.5 mb-1 ml-5 flex flex-col gap-0.5 border-l-2 border-neutral-200 pl-3"
      >
        <div v-for="tc in g.items" :key="tc.id">
          <div class="flex items-center gap-1.5 text-xs leading-5">
            <span class="flex h-3.5 w-3.5 shrink-0 items-center justify-center">
              <LoaderCircle
                v-if="tc.status === 'running'"
                class="h-3 w-3 animate-spin text-blue-500"
              />
              <X v-else-if="tc.status === 'error'" class="h-3 w-3 text-red-500" />
              <Check v-else class="h-3 w-3 text-emerald-500" />
            </span>
            <code
              class="min-w-0 flex-1 truncate rounded bg-neutral-100 px-1.5 py-0.5 font-mono text-[11px] text-neutral-600"
            >{{ itemSummary(tc) || tc.name }}</code>
          </div>
          <pre
            v-if="tc.result"
            class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-md border border-neutral-200 bg-neutral-50/60 p-2 font-mono text-[11px] leading-5 text-neutral-500"
          >{{ tc.result }}</pre>
        </div>
      </div>
    </div>
  </div>
</template>
