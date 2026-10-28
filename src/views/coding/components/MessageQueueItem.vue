<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { ArrowUp, Check, Pencil, X, ListOrdered } from 'lucide-vue-next'
import type { QueuedMessage } from '../../../composables/useMessageQueue'

const props = defineProps<{
  item: QueuedMessage
  index: number
  editable: boolean
}>()

const emit = defineEmits<{
  moveUp: [id: string]
  moveToFront: [id: string]
  edit: [id: string, content: string]
  remove: [id: string]
}>()

const editing = ref(false)
const editText = ref('')
const editEl = ref<HTMLTextAreaElement | null>(null)

/** 队列行只显示单行摘要：多行提示词在队列里占好几屏反而看不清顺序 */
const summary = computed(() => props.item.content.replace(/\s+/g, ' ').trim())

function startEdit() {
  if (!props.editable) return
  editing.value = true
  editText.value = props.item.content
  void nextTick(() => {
    editEl.value?.focus()
    editEl.value?.select()
  })
}

function commitEdit() {
  if (!editing.value) return
  // 先退出编辑态再 emit：Enter 提交后 textarea 卸载会触发 blur，若状态还开着
  // 就会二次 emit（同一条消息改两遍），且取消按钮的 click 也会先被 blur 提交掉。
  editing.value = false
  const text = editText.value.trim()
  // 空内容不提交：与 enqueue 的守卫一致，避免 flush 时白跑一轮
  if (text) emit('edit', props.item.id, text)
}

function cancelEdit() {
  editing.value = false
}

function onEditKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    cancelEdit()
    return
  }
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    commitEdit()
  }
}

// 生成开始（editable 变 false）时强制退出编辑态：flush 即将取走队首，
// 让用户对着一条正在被发送的消息改文本只会造成错觉。
watch(
  () => props.editable,
  (next) => {
    if (!next) editing.value = false
  },
)

// 该行在编辑中被移除（flush 取走 / 用户删除）时组件会卸载，不必额外处理；
// 但 blur 不会在卸载时触发，故不清理由这里兜底 —— 卸载即编辑结束。
</script>

<template>
  <div
    class="group flex items-start gap-2 px-3 py-2 text-xs transition-colors hover:bg-neutral-50"
    :class="index === 0 && 'bg-neutral-50/60'"
  >
    <span
      class="mt-0.5 w-4 shrink-0 text-right tabular-nums text-neutral-400"
      :title="index === 0 ? '下一个发送' : `队列第 ${index + 1} 条`"
    >{{ index + 1 }}</span>

    <template v-if="!editing">
      <span class="min-w-0 flex-1 break-words leading-5 text-neutral-700">{{ summary }}</span>
      <span
        v-if="index === 0"
        class="mt-0.5 shrink-0 rounded bg-neutral-900 px-1.5 py-0.5 text-[10px] leading-none text-white"
      >下一个发送</span>
    </template>
    <textarea
      v-else
      ref="editEl"
      v-model="editText"
      rows="2"
      class="min-w-0 flex-1 resize-none rounded-md border border-neutral-300 bg-white px-2 py-1 text-xs leading-5 text-neutral-800 outline-none focus:border-neutral-500"
      @keydown="onEditKeydown"
      @blur="commitEdit"
    />

    <div class="flex shrink-0 items-center gap-0.5">
      <template v-if="editing">
        <button
          class="rounded p-1 text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-neutral-800"
          title="保存"
          @mousedown.prevent
          @click="commitEdit"
        >
          <Check class="h-3.5 w-3.5" />
        </button>
        <button
          class="rounded p-1 text-neutral-500 transition-colors hover:bg-neutral-200 hover:text-neutral-800"
          title="取消"
          @mousedown.prevent
          @click="cancelEdit"
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </template>
      <template v-else>
        <button
          v-if="index > 0"
          class="rounded p-1 text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-700"
          title="上移一位"
          @click="emit('moveUp', item.id)"
        >
          <ArrowUp class="h-3.5 w-3.5" />
        </button>
        <button
          v-if="index > 0"
          class="rounded p-1 text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-700"
          title="移到队首，本轮结束后优先发送"
          @click="emit('moveToFront', item.id)"
        >
          <ListOrdered class="h-3.5 w-3.5" />
        </button>
        <button
          v-if="editable"
          class="rounded p-1 text-neutral-400 transition-colors hover:bg-neutral-200 hover:text-neutral-700"
          title="编辑"
          @click="startEdit"
        >
          <Pencil class="h-3.5 w-3.5" />
        </button>
        <button
          class="rounded p-1 text-neutral-400 transition-colors hover:bg-red-50 hover:text-red-600"
          title="从队列移除"
          @click="emit('remove', item.id)"
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </template>
    </div>
  </div>
</template>
