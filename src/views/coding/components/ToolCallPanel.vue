<script setup lang="ts">
// 工具活动列表：按**工具类别**分组成可折叠小节（设计参考图）。
//
// 为什么分组：一套连续调用常常是「读 3 个文件 → 改 2 个文件 → 跑 1 条命令」，
// 逐条平铺会把一屏塞满，且同类条目文字高度重复。按类别收成
// 「编辑了文件 · 3 个」「运行了命令 · 1 个」，展开才看每一条。
//
// 类别从 agent 给的 `title` 推断（`name` 存的是 ACP toolCall.title，例如
// 「运行了命令」「编辑了文件」「已读取 xxx」），推断不出来归入「其他」。
import { computed, ref } from 'vue'
import { ChevronRight, ChevronDown, LoaderCircle, CheckCircle2, XCircle } from 'lucide-vue-next'
import type { ToolCall } from '../../../types/session'

const props = defineProps<{
  toolCalls: ToolCall[]
  /** 所属轮次是否仍在生成（决定分组小节是否默认展开） */
  running?: boolean
}>()

/** 工具类别 */
type ToolGroupKey = 'command' | 'edit' | 'read' | 'search' | 'other'

interface ToolGroup {
  key: ToolGroupKey
  /** 小节标题，如「运行了命令」 */
  label: string
  items: ToolCall[]
}

/**
 * title → 类别。
 *
 * 只看 `name`（agent 给的 toolCall.title），**不看 input** —— input 是命令文本
 * 或文件路径，里面出现 "read"/"edit" 之类的字会被误判（实测：「已运行 read
 * C:/…」实际是读文件，却被 input 里的 read 拉去 command）。
 *
 * 顺序按「动词最具体」到最宽泛：先 read/edit/search，最后才 command。
 * 把 command 放最后是因为它最容易被别的词带偏（如「运行了 read」）。
 */
function classify(tc: ToolCall): ToolGroupKey {
  const t = tc.name.toLowerCase()
  if (/(读取|已读|查看|read|cat|open)/.test(t)) return 'read'
  if (/(编辑|修改|写入|write|edit|patch|apply)/.test(t)) return 'edit'
  if (/(搜索|查找|grep|search|find|glob)/.test(t)) return 'search'
  if (/(运行|执行|命令|command|bash|shell|terminal|exec)/.test(t)) return 'command'
  return 'other'
}

const GROUP_LABEL: Record<ToolGroupKey, string> = {
  command: '运行了命令',
  edit: '编辑了文件',
  read: '读取了文件',
  search: '搜索了内容',
  other: '其他操作',
}

/** 保持类别首次出现的顺序（不按固定枚举重排，避免与对话时序错位） */
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

/**
 * 每个分组的展开态。
 *
 * 默认折叠（过程信息不该抢占视线）；但当整轮**仍在运行**时展开最后一个
 * 有 running 条目的分组，让用户能看到实时进度。
 */
const expandedGroups = ref<Record<string, boolean>>({})

function isExpanded(g: ToolGroup): boolean {
  const explicit = expandedGroups.value[g.key]
  if (explicit !== undefined) return explicit
  // 未手动切换过：运行中自动展开「含在跑条目」的分组
  if (props.running && g.items.some((i) => i.status === 'running')) return true
  return false
}

function toggle(g: ToolGroup) {
  expandedGroups.value = { ...expandedGroups.value, [g.key]: !isExpanded(g) }
}

/** 小节右侧的状态点：有在跑就转圈，否则有失败就红点，否则绿勾 */
function groupStatus(g: ToolGroup): 'running' | 'error' | 'done' {
  if (g.items.some((i) => i.status === 'running')) return 'running'
  if (g.items.some((i) => i.status === 'error')) return 'error'
  return 'done'
}

/** 条目副标题：优先命令/路径摘要（input），过长由容器截断 */
function itemSummary(tc: ToolCall): string {
  const raw = tc.input?.trim() ?? ''
  if (!raw) return ''
  return raw.replace(/\s+/g, ' ').slice(0, 200)
}
</script>

<template>
  <div class="flex flex-col gap-0.5">
    <div v-for="g in groups" :key="g.key">
      <!-- 分组头：▸ 编辑了文件 · 3 个 -->
      <button
        class="flex w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-1 text-xs text-neutral-600 transition-colors hover:bg-neutral-100 hover:text-neutral-800"
        @click="toggle(g)"
      >
        <ChevronDown v-if="isExpanded(g)" class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
        <ChevronRight v-else class="h-3.5 w-3.5 shrink-0 text-neutral-400" />

        <LoaderCircle
          v-if="groupStatus(g) === 'running'"
          class="h-3.5 w-3.5 shrink-0 animate-spin text-blue-500"
        />
        <CheckCircle2
          v-else-if="groupStatus(g) === 'done'"
          class="h-3.5 w-3.5 shrink-0 text-neutral-300"
        />
        <XCircle v-else class="h-3.5 w-3.5 shrink-0 text-red-500" />

        <span class="shrink-0">{{ g.label }}</span>
        <span class="text-neutral-400">· {{ g.items.length }} 个</span>
      </button>

      <!-- 分组内容：每条一行，命令/路径置灰等宽 -->
      <div v-if="isExpanded(g)" class="mt-0.5 mb-1 ml-5 flex flex-col gap-0.5 border-l-2 border-neutral-200 pl-3">
        <div v-for="tc in g.items" :key="tc.id">
          <div class="flex items-center gap-1.5 text-xs leading-5">
            <LoaderCircle
              v-if="tc.status === 'running'"
              class="h-3 w-3 shrink-0 animate-spin text-blue-500"
            />
            <CheckCircle2
              v-else-if="tc.status === 'done'"
              class="h-3 w-3 shrink-0 text-green-600"
            />
            <XCircle v-else class="h-3 w-3 shrink-0 text-red-500" />

            <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-neutral-500">
              {{ itemSummary(tc) || tc.name }}
            </span>
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
