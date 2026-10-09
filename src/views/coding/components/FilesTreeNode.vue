<script setup lang="ts">
// 文件树节点（递归组件）：目录行 + 懒加载子级 / 文件行（双击打开）。
// 展开、缓存、loading、错误状态经 filesTreeState 控制器（inject）共享。
import { inject } from 'vue'
import {
  Folder,
  FolderOpen,
  FileCode2,
  FileText,
  File,
  ChevronRight,
  LoaderCircle,
  RefreshCw,
} from 'lucide-vue-next'
import type { DirEntry } from '../../../types/project'
import { fileIconKind } from '../../../utils/file'
import { treeControllerKey } from './filesTreeState'

const props = defineProps<{ entry: DirEntry; absPath: string; depth: number }>()

const tree = inject(treeControllerKey)

function fileIcon(name: string) {
  const kind = fileIconKind(name)
  if (kind === 'code') return FileCode2
  if (kind === 'text') return FileText
  return File
}

function onRowClick() {
  if (props.entry.type === 'dir') tree?.toggle(props.absPath)
}

function onFileDblClick() {
  if (props.entry.type === 'file') tree?.openFile(props.absPath, props.entry.name)
}
</script>

<template>
  <div>
    <!-- 节点行 -->
    <div
      class="flex cursor-pointer items-center gap-1.5 rounded-md py-1 pr-2 text-[13px] text-neutral-600 hover:bg-neutral-100"
      :style="{ paddingLeft: `${depth * 14 + 8}px` }"
      :title="entry.type === 'file' ? `双击打开 ${entry.name}` : entry.name"
      @click="onRowClick"
      @dblclick="onFileDblClick"
    >
      <template v-if="entry.type === 'dir'">
        <ChevronRight
          class="h-3 w-3 shrink-0 text-neutral-400 transition-transform"
          :class="tree?.isExpanded(absPath) && 'rotate-90'"
        />
        <component
          :is="tree?.isExpanded(absPath) ? FolderOpen : Folder"
          class="h-4 w-4 shrink-0 text-neutral-500"
        />
      </template>
      <template v-else>
        <!-- 文件行占位缩进：与目录行的箭头对齐 -->
        <span class="w-3 shrink-0" />
        <component :is="fileIcon(entry.name)" class="h-4 w-4 shrink-0 text-neutral-400" />
      </template>
      <span class="min-w-0 flex-1 truncate">{{ entry.name }}</span>
    </div>

    <!-- 目录子级：loading spin / 错误行内提示 + 重试 / 空占位 / 递归节点 -->
    <template v-if="entry.type === 'dir' && tree?.isExpanded(absPath)">
      <div
        v-if="tree.isLoading(absPath)"
        class="flex items-center gap-1.5 py-1 pr-2 text-[11px] text-neutral-400"
        :style="{ paddingLeft: `${(depth + 1) * 14 + 8}px` }"
      >
        <LoaderCircle class="h-3 w-3 animate-spin text-blue-500" />
        加载中…
      </div>
      <div
        v-else-if="tree.errorOf(absPath)"
        class="flex items-center gap-1.5 py-1 pr-2 text-[11px] text-red-500"
        :style="{ paddingLeft: `${(depth + 1) * 14 + 8}px` }"
      >
        <span class="min-w-0 flex-1 truncate">{{ tree.errorOf(absPath) }}</span>
        <button
          class="flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-200 hover:text-neutral-600"
          title="重试"
          @click.stop="tree.retry(absPath)"
        >
          <RefreshCw class="h-3 w-3" />
        </button>
      </div>
      <template v-else-if="(tree.entriesOf(absPath) ?? []).length > 0">
        <FilesTreeNode
          v-for="child in tree.entriesOf(absPath)"
          :key="child.name"
          :entry="child"
          :abs-path="tree.joinPath(absPath, child.name)"
          :depth="depth + 1"
        />
      </template>
      <p
        v-else
        class="py-1 text-[11px] text-neutral-400"
        :style="{ paddingLeft: `${(depth + 1) * 14 + 8}px` }"
      >
        空
      </p>
    </template>
  </div>
</template>
