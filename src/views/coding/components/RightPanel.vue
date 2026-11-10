<script setup lang="ts">
import { computed, defineAsyncComponent, watch } from 'vue'
import { Bot, FolderOpen, GitBranch, Info } from 'lucide-vue-next'
import type { Session } from '../../../types/session'
import { useDelegationStore } from '../../../stores/delegation'
import { useSessionStore } from '../../../stores/session'
import { useWorkspaceStore, type RightTab } from '../../../stores/workspace'
import FilesTree from './FilesTree.vue'
import SessionInspector from './SessionInspector.vue'
import DelegationTaskList from './DelegationTaskList.vue'
import DelegatedChildView from './DelegatedChildView.vue'
import EditorLoading from './EditorLoading.vue'
import AsyncPanelError from './AsyncPanelError.vue'
import { loadDiffViewer } from '../lazyWorkspace'

const props = defineProps<{ session: Session | null }>()

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
const delegation = useDelegationStore()

// 变更页签内含 Monaco：异步加载，避免工作区首屏静态拉入编辑器核心。
const DiffViewer = defineAsyncComponent({
  loader: loadDiffViewer,
  loadingComponent: EditorLoading,
  errorComponent: AsyncPanelError,
  delay: 0,
  timeout: 30_000,
})

/** 正在查看的子任务详情（来自拆出的子会话抽屉）。 */
const childSession = computed(() => sessionStore.get(workspace.delegatedChildSessionId))

/** 当前活动会话派生的子任务数（R6：只统计当前会话，详情态也沿用该计数）。 */
const subtaskCount = computed(() => {
  const id = props.session?.id
  if (!id) return 0
  return delegation.records.filter((record) => record.parentSessionId === id).length
})
const hasSubtasks = computed(() => subtaskCount.value > 0)
/** 有记录或正在看详情时才出现「子任务」页签（R2）。 */
const showSubtasks = computed(() => hasSubtasks.value || !!childSession.value)

const tabs = computed<{ key: RightTab; label: string; icon: typeof Info; count?: number }[]>(() => {
  const list: { key: RightTab; label: string; icon: typeof Info; count?: number }[] = [
    { key: 'files', label: '文件', icon: FolderOpen },
    { key: 'changes', label: '变更', icon: GitBranch },
    { key: 'detail', label: '详情', icon: Info },
  ]
  if (showSubtasks.value) {
    list.push({ key: 'subtasks', label: '子任务', icon: Bot, count: subtaskCount.value || undefined })
  }
  return list
})

// 页签不可用时回落「文件」：切到没有子任务的会话、或返回列表后记录已被清理。
watch(
  [() => workspace.rightTab, hasSubtasks, childSession],
  ([tab, has, child]) => {
    if (tab === 'subtasks' && !has && !child) workspace.rightTab = 'files'
  },
  { immediate: true },
)
</script>

<template>
  <aside class="flex h-full w-full flex-col border-l border-neutral-200 bg-white">
    <!-- 页签：文件 / 变更 / 详情 / 子任务（子任务按需出现） -->
    <div class="flex h-9 shrink-0 items-center gap-1 border-b border-neutral-200 px-2">
      <button
        v-for="t in tabs"
        :key="t.key"
        class="flex h-6.5 cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-xs font-medium"
        :class="workspace.rightTab === t.key ? 'bg-blue-50 text-blue-600' : 'text-neutral-500 hover:bg-neutral-100'"
        @click="workspace.rightTab = t.key"
      >
        <component :is="t.icon" class="h-3.5 w-3.5" />
        {{ t.label }}
        <span
          v-if="t.count"
          class="rounded-full px-1 text-[10px] tabular-nums"
          :class="workspace.rightTab === t.key ? 'bg-blue-100 text-blue-600' : 'bg-neutral-100 text-neutral-500'"
        >
          {{ t.count }}
        </span>
      </button>
    </div>

    <div class="min-h-0 flex-1">
      <template v-if="workspace.rightTab === 'subtasks'">
        <DelegatedChildView v-if="childSession" :session="childSession" />
        <DelegationTaskList v-else-if="session" :parent-session-id="session.id" />
        <p v-else class="p-4 text-[11px] text-neutral-400">当前没有活动会话</p>
      </template>
      <FilesTree v-else-if="workspace.rightTab === 'files'" />
      <DiffViewer v-else-if="workspace.rightTab === 'changes'" />
      <SessionInspector v-else-if="session" :session="session" />
      <p v-else class="p-4 text-[11px] text-neutral-400">当前没有活动会话</p>
    </div>
  </aside>
</template>
