<script setup lang="ts">
import { ref } from 'vue'
import { FolderOpen, GitBranch, Info } from 'lucide-vue-next'
import type { Session } from '../../../types/session'
import FilesTree from './FilesTree.vue'
import DiffViewer from './DiffViewer.vue'
import SessionInspector from './SessionInspector.vue'

defineProps<{ session: Session | null }>()

type RightTab = 'files' | 'changes' | 'detail'
const tab = ref<RightTab>('files')

const tabs: { key: RightTab; label: string; icon: typeof Info }[] = [
  { key: 'files', label: '文件', icon: FolderOpen },
  { key: 'changes', label: '变更', icon: GitBranch },
  { key: 'detail', label: '详情', icon: Info },
]
</script>

<template>
  <aside class="flex h-full w-full flex-col border-l border-neutral-200 bg-white">
    <!-- 页签：文件 / 变更 / 详情 -->
    <div class="flex h-9 shrink-0 items-center gap-1 border-b border-neutral-200 px-2">
      <button
        v-for="t in tabs"
        :key="t.key"
        class="flex h-6.5 cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-xs font-medium"
        :class="tab === t.key ? 'bg-blue-50 text-blue-600' : 'text-neutral-500 hover:bg-neutral-100'"
        @click="tab = t.key"
      >
        <component :is="t.icon" class="h-3.5 w-3.5" />
        {{ t.label }}
      </button>
    </div>

    <div class="min-h-0 flex-1">
      <FilesTree v-if="tab === 'files'" />
      <DiffViewer v-else-if="tab === 'changes'" />
      <SessionInspector v-else-if="session" :session="session" />
      <p v-else class="p-4 text-[11px] text-neutral-400">当前没有活动会话</p>
    </div>
  </aside>
</template>
