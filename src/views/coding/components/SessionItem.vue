<script setup lang="ts">
import { NPopconfirm } from 'naive-ui'
import { Pencil, Trash2, LoaderCircle, MessageSquare } from 'lucide-vue-next'
import type { Session } from '../../../types/session'
import { formatRelativeTime } from '../../../utils/time'
import { useWorkspaceStore } from '../../../stores/workspace'

const props = defineProps<{
  session: Session
  active: boolean
  renaming: boolean
  renameDraft: string
}>()

const emit = defineEmits<{
  select: []
  rename: []
  'commit-rename': []
  'cancel-rename': []
  remove: []
  'update:renameDraft': [value: string]
}>()

const workspace = useWorkspaceStore()
</script>

<template>
  <div
    class="group relative flex h-9 cursor-pointer items-center gap-2 rounded-md px-2.5 text-[13px]"
    :class="active ? 'bg-neutral-200/70 text-neutral-800' : 'text-neutral-600 hover:bg-neutral-200/50'"
    @click="emit('select')"
  >
    <!-- 重命名输入 -->
    <input
      v-if="renaming"
      :value="renameDraft"
      class="h-6 min-w-0 flex-1 rounded border border-neutral-300 bg-white px-1.5 text-xs outline-none focus:border-blue-400"
      autofocus
      @click.stop
      @input="emit('update:renameDraft', ($event.target as HTMLInputElement).value)"
      @keydown.enter="emit('commit-rename')"
      @keydown.esc="emit('cancel-rename')"
      @blur="emit('commit-rename')"
    />
    <template v-else>
      <LoaderCircle
        v-if="workspace.isGenerating(session.id)"
        class="h-3.5 w-3.5 shrink-0 animate-spin text-blue-500"
      />
      <MessageSquare v-else class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
      <span class="min-w-0 flex-1 truncate" :title="session.title">{{ session.title }}</span>
      <span class="shrink-0 text-[10px] text-neutral-400 group-hover:hidden">
        {{ formatRelativeTime(session.updatedAt) }}
      </span>
      <!-- 悬浮操作 -->
      <span class="hidden shrink-0 items-center gap-0.5 group-hover:flex" @click.stop>
        <button
          class="flex h-5 w-5 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-300/60 hover:text-neutral-600"
          title="重命名"
          @click="emit('rename')"
        >
          <Pencil class="h-3 w-3" />
        </button>
        <NPopconfirm @positive-click="emit('remove')">
          <template #trigger>
            <button
              class="flex h-5 w-5 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-300/60 hover:text-red-500"
              title="删除会话"
            >
              <Trash2 class="h-3 w-3" />
            </button>
          </template>
          删除会话「{{ session.title }}」？该操作不可撤销。
        </NPopconfirm>
      </span>
    </template>
  </div>
</template>
