<script setup lang="ts">
// 内容分区的渲染分发 —— 对齐 codeg 的 `ContentPartsRenderer`。
//
// 渲染层只认分区，不判断「哪条消息属于哪一段」（那是 utils/contentParts.ts 的事）。
// 五种分区：
//   text        正文（markdown）
//   reasoning   思考（折叠的浅色块）
//   tool-call   单条工具（独立卡片）
//   tool-group  连续工具（灰色胶囊，点开展开成卡片）
//   turn-error  错误卡
import { computed, ref } from 'vue'
import {
  AlertCircle,
  Brain,
  ChevronRight,
  FileDiff,
  FilePlus,
  FileText,
  RefreshCw,
} from 'lucide-vue-next'
import MarkdownContent from './MarkdownContent.vue'
import ToolCallPart from './ToolCallPart.vue'
import { baseName, groupSummary } from '../../../utils/contentParts'
import { useWorkspaceStore } from '../../../stores/workspace'
import { useProjectStore } from '../../../stores/project'
import type { ContentPart } from '../../../types/contentPart'

const props = defineProps<{
  part: ContentPart
  /** 是否允许错误卡显示「重试」（只有最新一轮可以） */
  allowRetry?: boolean
  /** 会话所属项目 —— 打开文件标签时需要它 */
  projectId?: string
}>()

const emit = defineEmits<{ retry: [] }>()

/**
 * 思考块的展开态。
 *
 * 默认折叠（过程信息），但正在流式输出时必须展开 —— 否则用户发完消息
 * 只看到一行「思考」，不知道 agent 有没有在动。
 */
const reasoningOpen = ref(false)
const reasoningExpanded = computed(
  () => reasoningOpen.value || (props.part.type === 'reasoning' && props.part.streaming),
)

const workspace = useWorkspaceStore()
const projectStore = useProjectStore()

/**
 * 点开一个文件：在编码工作区打开它的编辑标签。
 *
 * 与 FilesTree 走同一个 `workspace.openFile` —— 已打开则复用标签，不重复开。
 *
 * 路径处理（真实数据里 14/14 都是绝对路径，但不能只赌这一种）：
 * - 绝对路径（`C:\\…` 或 `/…`）直接用；
 * - 相对路径先拼上项目根目录，否则 `FileEditorView` 按 rootPath 读不到文件。
 *
 * 没有 projectId（临时会话 / 旧数据）时不响应，避免开出无处归属的标签。
 */
function openArtifact(rawPath: string): void {
  if (!props.projectId) return
  const project = projectStore.get(props.projectId)
  if (!project) return

  const path = isAbsolutePath(rawPath, project.rootPath)
    ? rawPath
    : joinPath(project.rootPath, rawPath)
  workspace.openFile(path, baseName(path), props.projectId)
}

/**
 * 绝对路径判定 —— **按项目根目录的平台风格**判断。
 *
 * 为什么不一律认 `/` 开头：在 Windows 上 `/out.txt` 是「当前盘根」，把它当绝对
 * 会拼丢项目根，开出读不到的文件。
 * 为什么又不能不认：POSIX 项目里 `/home/x/a.js` 就是绝对路径，当相对会把
 * 整个绝对路径再拼到根目录后面（实测拼出 `/home/me/proj/home/me/proj/out.txt`）。
 *
 * 所以用根目录本身来判断它属于哪种风格。
 */
function isAbsolutePath(path: string, rootPath: string): boolean {
  // Windows 盘符 / UNC：两种平台下都算绝对
  if (/^[a-zA-Z]:[\\/]/.test(path) || path.startsWith('\\\\')) return true
  // 裸 `/` 只在 POSIX 根目录下算绝对
  const rootIsPosix = !/^[a-zA-Z]:[\\/]/.test(rootPath) && !rootPath.startsWith('\\\\')
  return rootIsPosix && path.startsWith('/')
}

/**
 * 拼接项目根目录与相对路径。
 *
 * 分隔符按**根目录**的风格决定，且相对路径里的另一种分隔符要一并归一化 ——
 * 否则会拼出 `C:\proj\src/main.js` 这种混合风格，Windows 能读但
 * `tab.filePath` 用作标签 key 时与真正的同一文件对不上，导致重复开标签。
 */
function joinPath(root: string, rel: string): string {
  const sep = root.includes('\\') ? '\\' : '/'
  const trimmedRoot = root.replace(/[\\/]+$/, '')
  const trimmedRel = rel.replace(/^[\\/]+/, '')
  const normalizedRel =
    sep === '\\' ? trimmedRel.replace(/\//g, '\\') : trimmedRel.replace(/\\/g, '/')
  return `${trimmedRoot}${sep}${normalizedRel}`
}

/** 路径的目录部分（卡片第二行显示，帮助区分同名文件） */
function dirOf(path: string): string {
  const normalized = path.replace(/\\/g, '/')
  const idx = normalized.lastIndexOf('/')
  return idx > 0 ? normalized.slice(0, idx) : ''
}

/** 新增文件段的展开态（默认展开，对齐 codeg） */
const addedOpen = ref(true)
/** 改动文件段的展开态（默认折叠） */
const changedOpen = ref(false)

/** 工具胶囊的展开态 */
const groupOpen = ref(false)
const groupTitle = computed(() =>
  props.part.type === 'tool-group' ? groupSummary(props.part.items) : '',
)

/**
 * 改动段的合计增删行数。
 *
 * 只有「全部文件都能算出」时才给出合计数 —— 只要有一个文件是整文件写入
 * （`lineDelta === null`，没有基线），合计就会偏小，给出一个偏小的数字比
 * 不给更误导。此时返回 null，头部不显示 `+N -M`，各文件自己的数字仍在卡片内。
 */
const changedDelta = computed(() => {
  if (props.part.type !== 'reply-artifacts' || props.part.changed.length === 0) return null
  let added = 0
  let removed = 0
  for (const f of props.part.changed) {
    if (!f.lineDelta) return null
    added += f.lineDelta.added
    removed += f.lineDelta.removed
  }
  return { added, removed }
})

const isError = computed(() => props.part.type === 'turn-error')
const needsSettings = computed(
  () =>
    props.part.type === 'turn-error' &&
    (props.part.errorKind === 'dead' || props.part.errorKind === 'auth'),
)
</script>

<template>
  <!-- 正文：markdown，与相邻分区拉开间距（codeg 用 space-y-4） -->
  <MarkdownContent v-if="part.type === 'text'" :content="part.text" />

  <!-- 思考：`⟳ 思考 ›`（参考设计图，箭头在最右） -->
  <div v-else-if="part.type === 'reasoning'">
    <button
      class="flex w-full cursor-pointer items-center gap-1.5 rounded-md py-0.5 text-xs text-neutral-500 transition-colors hover:text-neutral-700"
      @click="reasoningOpen = !reasoningOpen"
    >
      <Brain class="h-3.5 w-3.5 shrink-0 opacity-60" />
      <span>{{ part.streaming ? '思考中' : '思考' }}</span>
      <span class="flex-1" />
      <svg
        class="h-3.5 w-3.5 shrink-0 opacity-50 transition-transform"
        :class="reasoningExpanded && 'rotate-90'"
        viewBox="0 0 16 16"
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        aria-hidden="true"
      >
        <path d="M6 3.5 10.5 8 6 12.5" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </button>
    <div
      v-if="reasoningExpanded"
      class="mt-1.5 whitespace-pre-wrap break-words border-l-2 border-neutral-200 pl-3 text-xs leading-5 text-neutral-500"
    >
      {{ part.text }}
    </div>
  </div>

  <!-- 单条工具：独立卡片 -->
  <ToolCallPart v-else-if="part.type === 'tool-call'" :part="part" class="max-w-full" />

  <!-- 连续工具：灰色圆角胶囊（对齐 codeg 的 ws-msg-chip） -->
  <div v-else-if="part.type === 'tool-group'" class="min-w-0 max-w-full">
    <button
      class="inline-flex max-w-full cursor-pointer items-center gap-1.5 rounded-lg bg-neutral-100 px-3 py-1.5 text-xs font-medium text-neutral-500 transition-colors hover:bg-neutral-200/70 hover:text-neutral-700"
      :title="groupOpen ? '收起' : '展开'"
      @click="groupOpen = !groupOpen"
    >
      <ChevronRight
        class="h-3 w-3 shrink-0 opacity-60 transition-transform"
        :class="groupOpen && 'rotate-90'"
      />
      <span class="min-w-0 truncate" :class="part.streaming && 'group-shimmer'">
        {{ groupTitle }}
      </span>
    </button>

    <!-- 展开后逐条显示工具卡 -->
    <div v-if="groupOpen" class="mt-3 space-y-3">
      <ToolCallPart v-for="item in part.items" :key="item.id" :part="item" />
    </div>
  </div>

  <!--
    文件改动汇总（对齐 codeg 的 replyArtifacts）：两段独立折叠，
    每个文件是**可点击的卡片**，点开在编码工作区打开该文件的编辑标签。
  -->
  <div v-else-if="part.type === 'reply-artifacts'" class="flex flex-col gap-2">
    <!-- 新增文件：默认展开（刚写的文件通常是想直接打开的） -->
    <div v-if="part.added.length > 0" class="overflow-hidden rounded-lg border border-neutral-200">
      <button
        class="flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-neutral-50"
        @click="addedOpen = !addedOpen"
      >
        <FilePlus class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
        <span class="shrink-0 text-xs font-medium text-neutral-700">新增文件</span>
        <span
          class="shrink-0 rounded border border-neutral-200 bg-neutral-50 px-1.5 py-0.5 text-[10px] text-neutral-500"
        >
          {{ part.added.length }} 个文件
        </span>
        <span class="flex-1" />
        <ChevronRight
          class="h-3.5 w-3.5 shrink-0 text-neutral-300 transition-transform"
          :class="addedOpen && 'rotate-90'"
        />
      </button>

      <div v-if="addedOpen" class="max-h-80 overflow-y-auto border-t border-neutral-100 p-2">
        <div class="grid gap-2 sm:grid-cols-2">
          <button
            v-for="f in part.added"
            :key="f.path"
            class="flex min-w-0 cursor-pointer items-center gap-2 rounded-md border border-emerald-600/30 bg-emerald-500/5 px-2.5 py-2 text-left transition-colors hover:border-emerald-600/50 hover:bg-emerald-500/10"
            :title="f.path"
            @click="openArtifact(f.path)"
          >
            <FileText class="h-4 w-4 shrink-0 text-neutral-400" />
            <span class="flex min-w-0 flex-1 flex-col">
              <span class="truncate text-xs font-medium text-neutral-800">
                {{ baseName(f.path) }}
              </span>
              <span v-if="dirOf(f.path)" class="truncate text-[10px] text-neutral-400">
                {{ dirOf(f.path) }}
              </span>
            </span>
          </button>
        </div>
      </div>
    </div>

    <!-- 改动文件：默认折叠 -->
    <div v-if="part.changed.length > 0" class="overflow-hidden rounded-lg border border-neutral-200">
      <button
        class="flex w-full cursor-pointer items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-neutral-50"
        @click="changedOpen = !changedOpen"
      >
        <FileDiff class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
        <span class="shrink-0 text-xs font-medium text-neutral-700">改动文件</span>
        <span
          class="shrink-0 rounded border border-neutral-200 bg-neutral-50 px-1.5 py-0.5 text-[10px] text-neutral-500"
        >
          {{ part.changed.length }} 个文件
        </span>

        <!-- 增删行数：只有局部替换能算，任一个整文件写入时整段不显示合计 -->
        <span
          v-if="changedDelta"
          class="shrink-0 rounded border border-neutral-200 bg-neutral-50 px-1.5 py-0.5 font-mono text-[10px] tabular-nums"
        >
          <span class="text-emerald-600">+{{ changedDelta.added }}</span>
          <span class="ml-1 text-red-500">-{{ changedDelta.removed }}</span>
        </span>

        <span class="flex-1" />
        <ChevronRight
          class="h-3.5 w-3.5 shrink-0 text-neutral-300 transition-transform"
          :class="changedOpen && 'rotate-90'"
        />
      </button>

      <div v-if="changedOpen" class="max-h-80 overflow-y-auto border-t border-neutral-100 p-2">
        <div class="grid gap-2 sm:grid-cols-2">
          <button
            v-for="f in part.changed"
            :key="f.path"
            class="flex min-w-0 cursor-pointer items-center gap-2 rounded-md border border-neutral-200 bg-white px-2.5 py-2 text-left transition-colors hover:border-neutral-300 hover:bg-neutral-50"
            :title="f.path"
            @click="openArtifact(f.path)"
          >
            <FileText class="h-4 w-4 shrink-0 text-neutral-400" />
            <span class="flex min-w-0 flex-1 flex-col">
              <span class="truncate text-xs font-medium text-neutral-800">
                {{ baseName(f.path) }}
              </span>
              <span v-if="dirOf(f.path)" class="truncate text-[10px] text-neutral-400">
                {{ dirOf(f.path) }}
              </span>
            </span>
            <span v-if="f.count > 1" class="shrink-0 font-mono text-[10px] text-neutral-400">
              ×{{ f.count }}
            </span>
          </button>
        </div>
      </div>
    </div>
  </div>

  <!-- 错误卡 -->
  <div
    v-else-if="isError"
    class="flex items-start gap-2 rounded-md border border-red-200 bg-red-50 px-3 py-2 text-[13px] text-red-600"
  >
    <AlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
    <span class="min-w-0 flex-1 break-words">{{ part.text || '生成失败' }}</span>
    <button
      v-if="allowRetry && !needsSettings"
      class="flex h-6 shrink-0 cursor-pointer items-center gap-1 rounded-md border border-red-200 bg-white px-2 text-xs text-red-600 hover:bg-red-100"
      title="重新生成"
      @click="emit('retry')"
    >
      <RefreshCw class="h-3 w-3" />
      重试
    </button>
  </div>
</template>

<style scoped>
/* 运行中的工具胶囊：文字扫光，与 codeg 的 Shimmer 同效果 */
.group-shimmer {
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
  animation: group-sweep 1.4s linear infinite;
}

@keyframes group-sweep {
  from {
    background-position: 100% center;
  }
  to {
    background-position: 0% center;
  }
}

@media (prefers-reduced-motion: reduce) {
  .group-shimmer {
    animation: none;
    color: inherit;
    -webkit-text-fill-color: currentColor;
  }
}
</style>
