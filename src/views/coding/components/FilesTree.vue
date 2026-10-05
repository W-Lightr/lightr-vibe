<script setup lang="ts">
import { computed, ref } from 'vue'
import { Folder, FolderOpen, FileCode2, FileText, File, ChevronRight } from 'lucide-vue-next'
import { useProjectStore } from '../../../stores/project'
import { useWorkspaceStore } from '../../../stores/workspace'
import { useSessionStore } from '../../../stores/session'
import { fileIconKind } from '../../../utils/file'

const projectStore = useProjectStore()
const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()

// 浏览当前会话所在工作区（项目）的文件树
const project = computed(() => {
  const session = sessionStore.get(workspace.activeSessionId)
  return projectStore.get(session?.projectId)
})

const expandedDirs = ref<Set<string>>(new Set(['.ccgui']))

function toggleDir(path: string) {
  if (expandedDirs.value.has(path)) expandedDirs.value.delete(path)
  else expandedDirs.value.add(path)
}

function fileIcon(name: string) {
  const kind = fileIconKind(name)
  if (kind === 'code') return FileCode2
  if (kind === 'text') return FileText
  return File
}

/** 双击文件：在中央栏新开编辑标签 */
function openFile(path: string, name: string) {
  if (project.value) workspace.openFile(path, name, project.value.id)
}
</script>

<template>
  <div class="h-full overflow-y-auto p-2">
    <template v-if="project">
      <div class="flex items-center gap-1.5 rounded-md px-2 py-1.5 text-[13px] text-neutral-700">
        <FolderOpen class="h-4 w-4 text-neutral-500" />
        {{ project.name }}
      </div>

      <template v-for="node in project.root" :key="node.name">
        <!-- 目录 -->
        <div v-if="node.type === 'dir'">
          <div
            class="flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1.5 text-[13px] text-neutral-600 hover:bg-neutral-100"
            @click="toggleDir(node.name)"
          >
            <ChevronRight
              class="h-3 w-3 shrink-0 text-neutral-400 transition-transform"
              :class="expandedDirs.has(node.name) && 'rotate-90'"
            />
            <Folder class="h-4 w-4 text-neutral-500" />
            <span class="truncate">{{ node.name }}</span>
          </div>
          <div v-if="expandedDirs.has(node.name)" class="ml-5">
            <div
              v-for="child in node.children"
              :key="`${node.name}/${child.name}`"
              class="flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1.5 text-[13px] text-neutral-600 hover:bg-neutral-100"
              :title="`双击打开 ${child.name}`"
              @dblclick="openFile(`${node.name}/${child.name}`, child.name)"
            >
              <component :is="fileIcon(child.name)" class="h-4 w-4 text-neutral-400" />
              <span class="truncate">{{ child.name }}</span>
            </div>
          </div>
        </div>

        <!-- 文件：双击在中央栏新开编辑标签 -->
        <div
          v-else
          class="ml-4 flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-1.5 text-[13px] text-neutral-600 hover:bg-neutral-100"
          :title="`双击打开 ${node.name}`"
          @dblclick="openFile(node.name, node.name)"
        >
          <component :is="fileIcon(node.name)" class="h-4 w-4 text-neutral-400" />
          <span class="truncate">{{ node.name }}</span>
        </div>
      </template>
    </template>

    <p v-else class="px-2 py-3 text-[11px] text-neutral-400">当前会话没有关联的工作区</p>
  </div>
</template>
