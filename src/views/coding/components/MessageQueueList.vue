<script setup lang="ts">
import { computed } from 'vue'
import { ListOrdered } from 'lucide-vue-next'
import MessageQueueItem from './MessageQueueItem.vue'
import {
  useQueue,
  remove,
  reorder,
  moveToFront,
  update,
  clear,
} from '../../../composables/useMessageQueue'

const props = defineProps<{
  sessionId: string
  /** 生成中时不允许编辑：编辑后的内容会立刻被 flush 取走，用户来不及确认 */
  generating: boolean
}>()

// useQueue 拿到的是模块级 reactive 容器的本体，flush 改动此处同步可见。
// 必须随 props.sessionId 重新取：Composer 切会话时本组件复用了同一实例，
// 若只在 setup 时取一次，界面会一直显示上一个会话的队列（AC9 串会话）。
const items = computed(() => useQueue(props.sessionId).items)

function onMoveUp(id: string) {
  reorder(props.sessionId, id, -1)
}

function onMoveToFront(id: string) {
  moveToFront(props.sessionId, id)
}

function onEdit(id: string, content: string) {
  update(props.sessionId, id, content)
}

function onRemove(id: string) {
  remove(props.sessionId, id)
}

function onClear() {
  clear(props.sessionId)
}
</script>

<template>
  <div
    v-if="items.length > 0"
    class="mx-3 mb-1.5 overflow-hidden rounded-2xl border border-neutral-200 bg-white shadow-sm sm:mx-6"
  >
    <div class="flex items-center gap-2 border-b border-neutral-100 px-3 py-1.5">
      <ListOrdered class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
      <span class="flex-1 text-xs font-medium text-neutral-600">发送队列 ({{ items.length }})</span>
      <button
        class="rounded px-1.5 py-0.5 text-[11px] text-neutral-400 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
        title="清空队列"
        @click="onClear"
      >清空</button>
    </div>
    <!-- 队列超过一屏时自身滚动，不把输入框顶出视口 -->
    <div class="max-h-52 overflow-y-auto">
      <MessageQueueItem
        v-for="(item, i) in items"
        :key="item.id"
        :item="item"
        :index="i"
        :editable="!generating"
        :class="i > 0 && 'border-t border-neutral-100'"
        @move-up="onMoveUp"
        @move-to-front="onMoveToFront"
        @edit="onEdit"
        @remove="onRemove"
      />
    </div>
  </div>
</template>
