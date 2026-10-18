<script setup lang="ts">
// 新建任务弹窗（最简版，PRD R3）：项目 + 标题 + 提示词。
// 子任务 3 会替换为完整 TaskEditorDialog（含引擎选择 / 编辑已有任务）；
// 这里保持「能真正建出一条任务」所需的最小字段，让看板与拖拽可被真实验证。
import { computed, ref, watch } from 'vue'
import { ChevronDown, X } from 'lucide-vue-next'
import { useProjectStore } from '../../../stores/project'

const props = defineProps<{
  open: boolean
  /** 初始项目（父级按当前筛选预置；null = 取第一个项目） */
  defaultProjectId: string | null
}>()

const emit = defineEmits<{
  close: []
  submit: [{ projectId: string; title: string; prompt: string }]
}>()

const projectStore = useProjectStore()

const projectId = ref<string | null>(null)
const title = ref('')
const prompt = ref('')
const dropOpen = ref(false)
const submitting = ref(false)

/** 打开时重置表单并预置项目 */
watch(
  () => props.open,
  (open) => {
    if (!open) return
    projectId.value = props.defaultProjectId ?? projectStore.projects[0]?.id ?? null
    title.value = ''
    prompt.value = ''
    dropOpen.value = false
    submitting.value = false
  },
)

const selectedProjectName = computed(
  () => projectStore.get(projectId.value)?.name ?? '选择项目',
)

const canSubmit = computed(
  () => projectId.value !== null && title.value.trim().length > 0 && !submitting.value,
)

function chooseProject(id: string): void {
  projectId.value = id
  dropOpen.value = false
}

function submit(): void {
  if (!canSubmit.value || projectId.value === null) return
  emit('submit', {
    projectId: projectId.value,
    title: title.value.trim(),
    prompt: prompt.value.trim(),
  })
}

/** Esc 关闭；Ctrl/Cmd+Enter 提交（textarea 里 Enter 要留给换行） */
function onKeydown(e: KeyboardEvent): void {
  if (e.key === 'Escape') {
    if (dropOpen.value) {
      dropOpen.value = false
      return
    }
    emit('close')
    return
  }
  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) submit()
}
</script>

<template>
  <div
    v-if="props.open"
    class="fixed inset-0 z-50 flex items-center justify-center bg-neutral-900/20"
    @mousedown.self="emit('close')"
    @keydown="onKeydown"
  >
    <div class="w-[420px] rounded-md border border-neutral-200 bg-white p-4">
      <div class="mb-3 flex items-center justify-between">
        <span class="text-[13px] font-medium text-neutral-800">新建任务</span>
        <button
          class="flex h-6 w-6 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
          title="关闭"
          @click="emit('close')"
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </div>

      <!-- 项目 -->
      <div class="mb-1.5 text-[11px] text-neutral-500">项目</div>
      <div class="relative">
        <button
          class="flex w-full cursor-pointer items-center gap-2 rounded-md border border-neutral-300 bg-white px-2.5 py-2 text-left transition-colors hover:bg-neutral-50"
          :class="dropOpen && 'border-blue-400'"
          type="button"
          :disabled="projectStore.projects.length === 0"
          @click="dropOpen = !dropOpen"
        >
          <span class="min-w-0 flex-1 truncate text-[13px] text-neutral-800">
            {{ selectedProjectName }}
          </span>
          <ChevronDown
            class="h-3.5 w-3.5 shrink-0 text-neutral-400 transition-transform"
            :class="dropOpen && 'rotate-180'"
          />
        </button>
        <div
          v-if="dropOpen"
          class="absolute z-10 mt-1 max-h-60 w-full overflow-y-auto rounded-md border border-neutral-200 bg-white py-1"
        >
          <button
            v-for="p in projectStore.projects"
            :key="p.id"
            class="flex w-full cursor-pointer items-center gap-2 px-2.5 py-2 text-left transition-colors hover:bg-neutral-100"
            :class="projectId === p.id && 'bg-blue-50/60'"
            :title="p.rootPath"
            type="button"
            @click="chooseProject(p.id)"
          >
            <span class="min-w-0 flex-1 truncate text-[13px] text-neutral-800">{{ p.name }}</span>
          </button>
        </div>
      </div>
      <p v-if="projectStore.projects.length === 0" class="mt-1.5 text-[11px] text-neutral-400">
        还没有项目，请先在编码工作区打开一个文件夹
      </p>

      <!-- 标题 -->
      <div class="mt-3 mb-1.5 text-[11px] text-neutral-500">标题</div>
      <input
        v-model="title"
        class="w-full rounded-md border border-neutral-300 px-2.5 py-2 text-[13px] text-neutral-800 outline-none placeholder:text-neutral-400 focus:border-blue-400"
        placeholder="一句话说明要做什么"
        maxlength="120"
      />

      <!-- 提示词 -->
      <div class="mt-3 mb-1.5 text-[11px] text-neutral-500">提示词（发给 Agent 的正文）</div>
      <textarea
        v-model="prompt"
        rows="4"
        class="w-full resize-none rounded-md border border-neutral-300 px-2.5 py-2 text-[13px] leading-5 text-neutral-800 outline-none placeholder:text-neutral-400 focus:border-blue-400"
        placeholder="描述背景、期望结果与验收标准"
      />

      <div class="mt-4 flex justify-end gap-2">
        <button
          class="cursor-pointer rounded-md border border-neutral-200 px-3 py-1.5 text-[13px] text-neutral-600 transition-colors hover:bg-neutral-100"
          @click="emit('close')"
        >
          取消
        </button>
        <button
          class="cursor-pointer rounded-md bg-blue-500 px-3 py-1.5 text-[13px] text-white transition-colors hover:bg-blue-600 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="!canSubmit"
          @click="submit"
        >
          创建
        </button>
      </div>
    </div>
  </div>
</template>
