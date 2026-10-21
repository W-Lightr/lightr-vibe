<script setup lang="ts">
// 任务编辑器（prd R1，替换子任务 2 的临时新建弹窗）：项目 + 标题 + 提示词 + 引擎。
// 新建与编辑共用一套表单：`task` 为空 = 新建（可改项目），否则为编辑（项目锁定，
// 且只在 todo / failed 状态开放 —— 与 Rust task_update 的放行条件一致）。
// 校验：标题非空、必须选项目、提示词不能全空白（提示词是发给 Agent 的正文，空任务跑不出结果）。
import { computed, onMounted, ref, watch } from 'vue'
import { ChevronDown, X } from 'lucide-vue-next'
import { engineIcon } from '../../../utils/engineIcons'
import { useEngineStore } from '../../../stores/engine'
import { useProjectStore } from '../../../stores/project'
import { isTaskEditable } from '../../../types/task'
import type { EngineWire } from '../../../types/engine'
import type { WorkTask } from '../../../types/task'

const props = defineProps<{
  open: boolean
  /** 编辑的任务（null = 新建） */
  task: WorkTask | null
  /** 新建时的初始项目（父级按当前筛选预置；null = 取第一个项目） */
  defaultProjectId: string | null
  /**
   * 新建时项目是否锁定（从具体项目看板进入时该任务必然属于此项目，prd R4）。
   * 编辑态本来就锁项目；此开关只影响新建。
   */
  projectLocked?: boolean
}>()

const emit = defineEmits<{
  close: []
  /** 提交：新建与编辑同一个形状（projectId 仅新建时生效） */
  submit: [{ projectId: string; title: string; prompt: string; engineId: EngineWire }]
}>()

const projectStore = useProjectStore()
const engineStore = useEngineStore()

const projectId = ref<string | null>(null)
const title = ref('')
const prompt = ref('')
const engineId = ref<EngineWire>(engineStore.preselectedEngineId)
const projectDropOpen = ref(false)
const engineDropOpen = ref(false)
const submitting = ref(false)

const editing = computed(() => props.task !== null)

/** 编辑态或父级要求锁定（项目看板新建）时项目不可改 */
const projectLocked = computed(() => editing.value || props.projectLocked === true)

/**
 * 打开时重置表单：新建取默认项目 + 默认引擎；编辑回填任务字段。
 * 项目锁定由父级显式传入（projectLocked），默认项目不会被「取第一个项目」的兜底覆盖。
 */
watch(
  () => props.open,
  (open) => {
    if (!open) {
      // 关闭即清空提交锁：上一次提交后父级若因校验失败没关窗，再打开不能还是「提交中」
      submitting.value = false
      return
    }
    projectDropOpen.value = false
    engineDropOpen.value = false
    submitting.value = false
    if (props.task) {
      projectId.value = props.task.projectId
      title.value = props.task.title
      prompt.value = props.task.prompt
      engineId.value = props.task.engineId ?? engineStore.preselectedEngineId
    } else if (projectLocked.value) {
      // 锁定（项目看板新建）：必须用父级给的项目，不回落第一个，避免建到别的项目下
      projectId.value = props.defaultProjectId ?? null
      title.value = ''
      prompt.value = ''
      engineId.value = engineStore.preselectedEngineId
    } else {
      // 未锁定（卡片页 / 全部项目）：预置项目仅作初值，用户可改
      projectId.value = props.defaultProjectId ?? projectStore.projects[0]?.id ?? null
      title.value = ''
      prompt.value = ''
      engineId.value = engineStore.preselectedEngineId
    }
  },
  { immediate: true },
)

/** 引擎列表刷新（失败保留静态兜底，不阻塞填写） */
onMounted(() => {
  void engineStore.load()
})

const projectName = computed(() => projectStore.get(projectId.value)?.name ?? '选择项目')
const engineName = computed(
  () => engineStore.agents.find((a) => a.agentType === engineId.value)?.name ?? '选择引擎',
)

const canSubmit = computed(
  () =>
    projectId.value !== null &&
    title.value.trim().length > 0 &&
    prompt.value.trim().length > 0 &&
    !submitting.value,
)

function chooseProject(id: string): void {
  if (projectLocked.value) return
  projectId.value = id
  projectDropOpen.value = false
}

function chooseEngine(id: EngineWire): void {
  engineId.value = id
  engineDropOpen.value = false
}

function submit(): void {
  if (!canSubmit.value || projectId.value === null) return
  submitting.value = true
  emit('submit', {
    projectId: projectId.value,
    title: title.value.trim(),
    prompt: prompt.value.trim(),
    engineId: engineId.value,
  })
}

/** Esc 关闭（先收下拉）；Ctrl/Cmd+Enter 提交（textarea 里 Enter 留给换行） */
function onKeydown(e: KeyboardEvent): void {
  if (e.key === 'Escape') {
    if (projectDropOpen.value) {
      projectDropOpen.value = false
      return
    }
    if (engineDropOpen.value) {
      engineDropOpen.value = false
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
    data-no-drag
    class="fixed inset-0 z-50 flex items-center justify-center bg-neutral-900/20"
    @mousedown.self="emit('close')"
    @keydown="onKeydown"
  >
    <div class="w-[480px] rounded-md border border-neutral-200 bg-white p-4">
      <div class="mb-3 flex items-center justify-between">
        <span class="text-[13px] font-medium text-neutral-800">
          {{ editing ? '编辑任务' : '新建任务' }}
        </span>
        <button
          class="flex h-6 w-6 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
          title="关闭"
          @click="emit('close')"
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </div>

      <!-- 编辑态下任务不可改的提示（审核中 / 已完成等） -->
      <p
        v-if="editing && props.task && !isTaskEditable(props.task.status)"
        class="mb-3 rounded-md bg-amber-50 px-2.5 py-2 text-[11px] text-amber-700"
      >
        当前状态的任务不允许编辑（只有待办 / 失败可改）。
      </p>

      <!-- 项目 -->
      <div class="mb-1.5 text-[11px] text-neutral-500">项目</div>
      <div class="relative" @mousedown.stop>
        <button
          class="flex w-full cursor-pointer items-center gap-2 rounded-md border border-neutral-300 bg-white px-2.5 py-2 text-left transition-colors hover:bg-neutral-50 disabled:cursor-not-allowed disabled:bg-neutral-50"
          :class="projectDropOpen && 'border-blue-400'"
          type="button"
          :disabled="projectLocked || projectStore.projects.length === 0"
          @click="projectDropOpen = !projectDropOpen"
        >
          <span class="min-w-0 flex-1 truncate text-[13px] text-neutral-800">{{ projectName }}</span>
          <ChevronDown
            v-if="!projectLocked"
            class="h-3.5 w-3.5 shrink-0 text-neutral-400 transition-transform"
            :class="projectDropOpen && 'rotate-180'"
          />
        </button>
        <div
          v-if="projectDropOpen"
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
      <p
        v-else-if="!editing && projectLocked"
        class="mt-1.5 text-[11px] text-neutral-400"
      >
        任务将创建在当前项目中
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
        rows="5"
        class="w-full resize-none rounded-md border border-neutral-300 px-2.5 py-2 text-[13px] leading-5 text-neutral-800 outline-none placeholder:text-neutral-400 focus:border-blue-400"
        placeholder="描述背景、期望结果与验收标准"
      />

      <!-- 引擎 -->
      <div class="mt-3 mb-1.5 text-[11px] text-neutral-500">引擎</div>
      <div class="relative" @mousedown.stop>
        <button
          class="flex w-full cursor-pointer items-center gap-2 rounded-md border border-neutral-300 bg-white px-2.5 py-2 text-left transition-colors hover:bg-neutral-50"
          :class="engineDropOpen && 'border-blue-400'"
          type="button"
          @click="engineDropOpen = !engineDropOpen"
        >
          <span
            class="h-4 w-4 shrink-0 text-neutral-600 [&>svg]:h-4 [&>svg]:w-4"
            v-html="engineIcon(engineId)"
          />
          <span class="min-w-0 flex-1 truncate text-[13px] text-neutral-800">{{ engineName }}</span>
          <ChevronDown
            class="h-3.5 w-3.5 shrink-0 text-neutral-400 transition-transform"
            :class="engineDropOpen && 'rotate-180'"
          />
        </button>
        <div
          v-if="engineDropOpen"
          class="absolute z-10 mt-1 max-h-60 w-full overflow-y-auto rounded-md border border-neutral-200 bg-white py-1"
        >
          <button
            v-for="agent in engineStore.agents"
            :key="agent.agentType"
            class="flex w-full cursor-pointer items-center gap-2 px-2.5 py-2 text-left transition-colors hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-40"
            :class="engineId === agent.agentType && 'bg-blue-50/60'"
            :disabled="!agent.enabled"
            type="button"
            @click="chooseEngine(agent.agentType)"
          >
            <span
              class="h-4 w-4 shrink-0 text-neutral-600 [&>svg]:h-4 [&>svg]:w-4"
              v-html="engineIcon(agent.agentType)"
            />
            <span class="min-w-0 flex-1 truncate text-[13px] text-neutral-800">{{ agent.name }}</span>
            <span v-if="!agent.available" class="shrink-0 text-[10px] text-amber-500">未安装</span>
          </button>
        </div>
      </div>

      <div class="mt-4 flex items-center justify-end gap-2">
        <span class="mr-auto text-[11px] text-neutral-400">Ctrl / ⌘ + Enter 保存</span>
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
          {{ editing ? '保存' : '创建' }}
        </button>
      </div>
    </div>
  </div>
</template>
