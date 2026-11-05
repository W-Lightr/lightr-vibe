<script setup lang="ts">
// 文件树（懒加载）：浏览当前会话所在项目的真实目录。
// 展开目录时才请求 fs_list_dir，控制器缓存条目（design D2）；
// 根目录进入即加载；loading / 错误重试 / 空占位在 FilesTreeNode 内呈现。
import { computed, provide, ref, watch } from 'vue'
import { FolderOpen, LoaderCircle, RefreshCw } from 'lucide-vue-next'
import { useWorkspaceStore } from '../../../stores/workspace'
import { useSessionStore } from '../../../stores/session'
import { useProjectStore } from '../../../stores/project'
import { fsErrorMessage } from '../../../types/project'
import { prefetchFileEditor } from '../lazyWorkspace'
import FilesTreeNode from './FilesTreeNode.vue'
import { createTreeController, treeControllerKey } from './filesTreeState'

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
const projectStore = useProjectStore()

// 浏览当前会话所在工作区（项目）的文件树
const project = computed(() => {
  const session = sessionStore.get(workspace.activeSessionId)
  return projectStore.get(session?.projectId)
})

const controller = createTreeController(
  () => project.value?.rootPath ?? null,
  (absPath, name) => {
    if (project.value) workspace.openFile(absPath, name, project.value.id)
  },
)
provide(treeControllerKey, controller)

// 根目录状态：加载 spin / 错误提示 + 重试（子目录状态在 FilesTreeNode 内）
const rootError = computed(() => (project.value ? controller.errorOf(project.value.rootPath) : null))
const rootEntries = computed(() =>
  project.value ? controller.entriesOf(project.value.rootPath) : undefined,
)

const refreshing = ref(false)

/** 加载根目录（项目就绪或切换时；已缓存则跳过） */
function loadRoot() {
  if (project.value) void controller.loadDir(project.value.rootPath)
}

// 项目切换：清空全部缓存/展开态，重新加载新根
watch(
  () => project.value?.rootPath,
  () => {
    controller.reset()
    loadRoot()
  },
  { immediate: true },
)

async function refresh() {
  if (!project.value) return
  refreshing.value = true
  try {
    await controller.refreshAll()
  } catch (err) {
    // refreshAll 内部已按目录记录错误；此处仅兜底防御
    controller.errors.value.set(project.value.rootPath, fsErrorMessage(err))
  } finally {
    refreshing.value = false
  }
}
</script>

<template>
  <div class="flex h-full flex-col">
    <template v-if="project">
      <!-- 项目头：名称 + 刷新按钮 -->
      <div class="flex shrink-0 items-center gap-1.5 px-2 py-1.5 text-[13px] text-neutral-700">
        <FolderOpen class="h-4 w-4 shrink-0 text-neutral-500" />
        <span class="min-w-0 flex-1 truncate" :title="project.rootPath">{{ project.name }}</span>
        <button
          class="flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-200 hover:text-neutral-600"
          title="刷新文件树"
          @click="refresh"
        >
          <RefreshCw class="h-3 w-3" :class="refreshing && 'animate-spin'" />
        </button>
      </div>

      <!-- 悬停文件树即预取编辑器 chunk（Monaco），缩短首次打开文件的等待 -->
      <div class="min-h-0 flex-1 overflow-y-auto px-2 pb-2" @pointerenter="prefetchFileEditor">
        <!-- 根目录 loading -->
        <div
          v-if="controller.isLoading(project.rootPath)"
          class="flex items-center gap-1.5 px-2 py-1 text-[11px] text-neutral-400"
        >
          <LoaderCircle class="h-3 w-3 animate-spin text-blue-500" />
          加载中…
        </div>

        <!-- 根目录错误：行内红色提示 + 重试 -->
        <div
          v-else-if="rootError"
          class="flex items-center gap-1.5 px-2 py-1 text-[11px] text-red-500"
        >
          <span class="min-w-0 flex-1 truncate">{{ rootError }}</span>
          <button
            class="flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-200 hover:text-neutral-600"
            title="重试"
            @click="controller.retry(project.rootPath)"
          >
            <RefreshCw class="h-3 w-3" />
          </button>
        </div>

        <!-- 空项目 -->
        <p
          v-else-if="rootEntries && rootEntries.length === 0"
          class="px-2 py-1.5 text-[11px] text-neutral-400"
        >
          空文件夹
        </p>

        <!-- 顶层节点（递归） -->
        <template v-else-if="rootEntries">
          <FilesTreeNode
            v-for="entry in rootEntries"
            :key="entry.name"
            :entry="entry"
            :abs-path="controller.joinPath(project.rootPath, entry.name)"
            :depth="0"
          />
        </template>
      </div>
    </template>

    <p v-else class="px-2 py-3 text-[11px] text-neutral-400">当前会话没有关联的工作区</p>
  </div>
</template>
