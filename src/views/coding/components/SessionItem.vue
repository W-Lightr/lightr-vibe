<script setup lang="ts">
// 单个会话行：标题/时间/悬浮操作（重命名、删除）。删除确认自绘弹层（codeg 风格，
// Teleport 到 body + fixed 定位，避免被侧栏 overflow 裁剪）。
import { computed, onBeforeUnmount, ref } from 'vue'
import { Pencil, Trash2, LoaderCircle, MessageSquare } from 'lucide-vue-next'
import type { Session } from '../../../types/session'
import { formatRelativeTime } from '../../../utils/time'
import { engineIcon } from '../../../utils/engineIcons'
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

/** 会话引擎图标 SVG（内联）；未命中回退通用 MessageSquare 图标。 */
const engineIconSvg = computed(() => engineIcon(props.session.engineId))

/** 删除确认弹层开关（自绘，替代 naive-ui NPopconfirm；Teleport 到 body 防裁剪） */
const confirmOpen = ref(false)
const deleteBtnRef = ref<HTMLElement | null>(null)
/** 弹层 fixed 定位坐标（相对触发按钮） */
const confirmPos = ref<{ left: number; top: number }>({ left: 0, top: 0 })
const CONFIRM_WIDTH = 288

/** 打开确认弹层：按触发按钮矩形计算位置（右对齐按钮、弹层落在按钮上方） */
function openConfirm() {
  const el = deleteBtnRef.value
  if (!el) return
  const r = el.getBoundingClientRect()
  confirmPos.value = {
    left: Math.max(8, r.right - CONFIRM_WIDTH),
    top: r.top - 8,
  }
  confirmOpen.value = true
}

function toggleConfirm() {
  if (confirmOpen.value) closeConfirm()
  else openConfirm()
}

function closeConfirm() {
  confirmOpen.value = false
}

function confirmRemove() {
  confirmOpen.value = false
  emit('remove')
}

/** 点击弹层外部关闭 */
function onWindowMousedown(e: MouseEvent) {
  if (!confirmOpen.value) return
  const t = e.target as HTMLElement
  if (t.closest('[data-session-confirm]') || t.closest('[data-session-delete-btn]')) return
  closeConfirm()
}

window.addEventListener('mousedown', onWindowMousedown)
onBeforeUnmount(() => window.removeEventListener('mousedown', onWindowMousedown))
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
      <!-- 会话行显示所选引擎图标（opencode/claude/codex…）；未命中回退通用图标 -->
      <span
        v-else-if="engineIconSvg"
        class="h-3.5 w-3.5 shrink-0 text-neutral-500 [&>svg]:h-3.5 [&>svg]:w-3.5"
        v-html="engineIconSvg"
      />
      <MessageSquare v-else class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
      <span class="min-w-0 flex-1 truncate" :title="session.title">{{ session.title }}</span>
      <span class="shrink-0 text-[10px] text-neutral-400 group-hover:hidden">
        {{ formatRelativeTime(session.updatedAt) }}
      </span>
      <!-- 悬浮操作（确认弹层打开时保持可见，避免鼠标移入弹层后消失） -->
      <span
        class="hidden shrink-0 items-center gap-0.5 group-hover:flex"
        :class="confirmOpen && 'flex'"
        @click.stop
      >
        <button
          class="flex h-5 w-5 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-300/60 hover:text-neutral-600"
          title="重命名"
          @click="emit('rename')"
        >
          <Pencil class="h-3 w-3" />
        </button>
        <div>
          <button
            ref="deleteBtnRef"
            data-session-delete-btn
            class="flex h-5 w-5 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-300/60 hover:text-red-500"
            title="删除会话"
            @click="toggleConfirm"
          >
            <Trash2 class="h-3 w-3" />
          </button>
          <!-- 删除确认弹层（codeg 风格）：Teleport 到 body + fixed，逃出侧栏 overflow 裁剪 -->
          <Teleport to="body">
            <div
              v-if="confirmOpen"
              data-session-confirm
              class="fixed z-[100] w-72 -translate-y-full rounded-xl border border-neutral-200 bg-white p-4 shadow-lg"
              :style="{ left: `${confirmPos.left}px`, top: `${confirmPos.top}px` }"
            >
              <div class="text-[15px] font-semibold text-neutral-900">删除会话?</div>
              <p class="mt-1.5 text-[13px] leading-5 text-neutral-500">
                会删除「{{ session.title }}」，此操作不可撤销。
              </p>
              <div class="mt-4 flex justify-end gap-2">
                <button
                  class="cursor-pointer rounded-full bg-neutral-100 px-3.5 py-1.5 text-[13px] text-neutral-700 transition-colors hover:bg-neutral-200"
                  @click="closeConfirm"
                >
                  取消
                </button>
                <button
                  class="cursor-pointer rounded-full bg-neutral-900 px-3.5 py-1.5 text-[13px] text-white transition-colors hover:bg-neutral-700"
                  @click="confirmRemove"
                >
                  删除
                </button>
              </div>
            </div>
          </Teleport>
        </div>
      </span>
    </template>
  </div>
</template>
