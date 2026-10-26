<script setup lang="ts">
// 任务编辑器（prd R1）：标题 + 引擎图标横排 + 大提示词框（内含模型栏）+ 目标区 + 页脚。
// 新建与编辑共用一套表单：`task` 为空 = 新建（可改项目），否则为编辑（项目锁定，
// 且只在 todo / failed 状态开放 —— 与 Rust task_update 的放行条件一致）。
// 校验：标题非空、必须选项目、提示词不能全空白（提示词是发给 Agent 的正文，空任务跑不出结果）。
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ChevronDown, FolderOpen, X } from 'lucide-vue-next'
import { engineIcon } from '../../../utils/engineIcons'
import { acpService } from '../../../services/acpService'
import { useEngineStore } from '../../../stores/engine'
import { useProjectStore } from '../../../stores/project'
import { isTaskEditable } from '../../../types/task'
import type { EngineWire } from '../../../types/engine'
import type { AcpConfigOption, AcpConfigOptionGroup, AcpConfigOptionValue } from '../../../types/acp'
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
  submit: [
    {
      projectId: string
      title: string
      prompt: string
      engineId: EngineWire
      modelId: string | null
      /** 思考程度：config option 的 id 与所选值（引擎不支持时均为 null） */
      effortId: string | null
      effortValue: string | null
    },
  ]
}>()

const projectStore = useProjectStore()
const engineStore = useEngineStore()

const projectId = ref<string | null>(null)
const title = ref('')
const prompt = ref('')
const engineId = ref<EngineWire>(engineStore.preselectedEngineId)
const modelId = ref<string | null>(null)
const projectDropOpen = ref(false)
const modelDropOpen = ref(false)
const effortDropOpen = ref(false)
const submitting = ref(false)

/** 引擎探测出的模型选项（按当前引擎 + 项目路径拉取） */
const modelOptions = ref<AcpConfigOptionValue[]>([])
const modelLoading = ref(false)
const modelError = ref<string | null>(null)

/** 思考程度（ACP config option `category: 'effort'`）。
 *  各引擎支持情况不同：探测到该选项才显示下拉，没有就整块隐藏。 */
const effortId = ref<string | null>(null)
const effortOptions = ref<AcpConfigOptionValue[]>([])
const effortValue = ref<string | null>(null)

const editing = computed(() => props.task !== null)

/** 编辑态或父级要求锁定（项目看板新建）时项目不可改 */
const projectLocked = computed(() => editing.value || props.projectLocked === true)

/** 可用引擎（横向图标排用；启用且已安装的排前面） */
const engineChoices = computed(() =>
  engineStore.agents
    .filter((a) => a.enabled)
    .slice()
    .sort((a, b) => Number(b.available) - Number(a.available)),
)

const currentModelLabel = computed(
  () => modelOptions.value.find((m) => m.value === modelId.value)?.name ?? modelId.value ?? '默认模型',
)

/** 展平 select 的 options（支持 group 形态） */
function flattenOptions(option: AcpConfigOption | undefined): AcpConfigOptionValue[] {
  if (!option?.options) return []
  const flat: AcpConfigOptionValue[] = []
  for (const entry of option.options) {
    const group = entry as AcpConfigOptionGroup
    if (Array.isArray(group.options)) flat.push(...group.options)
    else flat.push(entry as AcpConfigOptionValue)
  }
  return flat
}

/** 按当前引擎 + 项目路径探测模型列表；失败只记录原因，不阻塞填写 */
async function loadModels(): Promise<void> {
  modelDropOpen.value = false
  effortDropOpen.value = false
  modelError.value = null
  // 换引擎 / 换项目后旧的 effort 不再适用，先清空；探测到才重建
  effortId.value = null
  effortOptions.value = []
  effortValue.value = null
  const cwd = projectStore.get(projectId.value)?.rootPath
  if (!cwd) {
    modelOptions.value = []
    return
  }
  modelLoading.value = true
  try {
    const options = await acpService.probeOptions(cwd, engineId.value)
    const modelOption = options?.find((o) => o.category === 'model') ??
      options?.find((o) => o.id === 'model')
    modelOptions.value = flattenOptions(modelOption)
    // 当前值不在候选里（换了引擎 / 探测失败）时回落到探测到的当前值
    if (modelOption?.currentValue != null) {
      const current = String(modelOption.currentValue)
      if (!modelId.value || !modelOptions.value.some((m) => m.value === modelId.value)) {
        modelId.value = current
      }
    }
    // 思考程度：各引擎支持不同（opencode 无此项），探测到 category=effort 才显示
    const effortOption = options?.find((o) => o.category === 'effort') ??
      options?.find((o) => o.id === 'effort')
    const effortCandidates = flattenOptions(effortOption)
    if (effortOption && effortCandidates.length > 0) {
      effortId.value = effortOption.id
      effortOptions.value = effortCandidates
      const current = effortOption.currentValue != null ? String(effortOption.currentValue) : null
      effortValue.value =
        current && effortCandidates.some((o) => o.value === current)
          ? current
          : (effortCandidates[0]?.value ?? null)
    }
  } catch {
    modelOptions.value = []
    modelError.value = '未能获取模型列表'
  } finally {
    modelLoading.value = false
  }
}

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
    modelDropOpen.value = false
    effortDropOpen.value = false
    submitting.value = false
    modelId.value = null
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
    void loadModels()
  },
  { immediate: true },
)

/** 引擎列表刷新（失败保留静态兜底，不阻塞填写）。
 *  同时挂 document 的捕获阶段点击监听，用于「点外部收起下拉」。 */
onMounted(() => {
  void engineStore.load()
  document.addEventListener('mousedown', onDocumentPointerDown, true)
})

onBeforeUnmount(() => {
  document.removeEventListener('mousedown', onDocumentPointerDown, true)
})

const projectName = computed(() => projectStore.get(projectId.value)?.name ?? '选择项目')

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
  // 项目决定工作目录，模型候选随之变化
  modelId.value = null
  void loadModels()
}

function chooseEngine(id: EngineWire): void {
  if (engineId.value === id) return
  engineId.value = id
  // 换引擎后模型候选完全不同：清掉旧选择并按新引擎重新探测
  modelId.value = null
  void loadModels()
}

function chooseModel(id: string): void {
  modelId.value = id
  modelDropOpen.value = false
}

function chooseEffort(id: string): void {
  effortValue.value = id
  effortDropOpen.value = false
}

function submit(): void {
  if (!canSubmit.value || projectId.value === null) return
  submitting.value = true
  emit('submit', {
    projectId: projectId.value,
    title: title.value.trim(),
    prompt: prompt.value.trim(),
    engineId: engineId.value,
    modelId: modelId.value,
    effortId: effortId.value,
    effortValue: effortValue.value,
  })
}

/** Esc 关闭（先收下拉）；Ctrl/Cmd+Enter 提交（textarea 里 Enter 留给换行） */
function onKeydown(e: KeyboardEvent): void {
  if (e.key === 'Escape') {
    if (projectDropOpen.value) {
      projectDropOpen.value = false
      return
    }
    if (modelDropOpen.value) {
      modelDropOpen.value = false
      return
    }
    if (effortDropOpen.value) {
      effortDropOpen.value = false
      return
    }
    emit('close')
    return
  }
  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) submit()
}

/** 点击面板外部收起下拉。
 *  触发器与面板都带 `@mousedown.stop`（防止点击面板本身触发遮罩关闭），
 *  因此这里在 **capture 阶段**监听 document：无论冒泡是否被拦，都能收到点击，
 *  再按 `closest('[data-dropdown]')` 判断是否点在某个下拉内部。 */
function onDocumentPointerDown(e: MouseEvent): void {
  const target = e.target
  if (!(target instanceof Element)) return
  if (target.closest('[data-dropdown]')) return
  projectDropOpen.value = false
  modelDropOpen.value = false
  effortDropOpen.value = false
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
    <div class="flex max-h-[85vh] w-[600px] flex-col rounded-lg border border-neutral-200 bg-white">
      <!-- 头部：标题 + 关闭 -->
      <div class="flex shrink-0 items-center justify-between px-5 pt-4 pb-3">
        <span class="text-[14px] font-medium text-neutral-800">
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

      <div class="min-h-0 flex-1 overflow-y-auto px-5 pb-4">
        <!-- 编辑态下任务不可改的提示（审核中 / 已完成等） -->
        <p
          v-if="editing && props.task && !isTaskEditable(props.task.status)"
          class="mb-3 rounded-md bg-amber-50 px-2.5 py-2 text-[11px] text-amber-700"
        >
          当前状态的任务不允许编辑（只有待办 / 失败可改）。
        </p>

        <!-- 标题：无边框大字号，占据视觉重心（图二） -->
        <input
          v-model="title"
          class="w-full border-0 bg-transparent p-0 text-[18px] font-medium text-neutral-800 outline-none placeholder:text-neutral-400"
          placeholder="要做什么？"
          maxlength="120"
        />

        <!-- 引擎：横向图标排 + 当前引擎名（图二） -->
        <div class="mt-4 flex flex-wrap items-center gap-1.5">
          <button
            v-for="agent in engineChoices"
            :key="agent.agentType"
            class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-md border transition-colors disabled:cursor-not-allowed disabled:opacity-40"
            :class="
              engineId === agent.agentType
                ? 'border-neutral-300 bg-neutral-100'
                : 'border-transparent hover:bg-neutral-100'
            "
            :title="agent.available ? agent.name : `${agent.name}（未安装）`"
            :disabled="!agent.available"
            type="button"
            @click="chooseEngine(agent.agentType)"
          >
            <span
              class="h-4 w-4 text-neutral-700 [&>svg]:h-4 [&>svg]:w-4"
              v-html="engineIcon(agent.agentType)"
            />
          </button>
          <span class="ml-1 min-w-0 truncate text-[12px] text-neutral-500">
            {{ engineStore.agents.find((a) => a.agentType === engineId)?.name ?? '选择引擎' }}
          </span>
        </div>

        <!-- 提示词框：大文本域，底部一行是模型栏（图二） -->
        <div class="mt-3 rounded-lg border border-neutral-200 transition-colors focus-within:border-neutral-400">
          <textarea
            v-model="prompt"
            rows="6"
            class="block w-full resize-none rounded-t-lg px-3 py-2.5 text-[13px] leading-5 text-neutral-800 outline-none placeholder:text-neutral-400"
            placeholder="向 agent 描述任务"
          />
          <!-- 模型栏：+ 图标 + 模型下拉（按引擎探测）+ 思考程度（有则显示）+ 刷新 -->
          <div class="flex items-center gap-1 border-t border-neutral-100 px-2 py-1.5">
            <div class="relative" data-dropdown @mousedown.stop>
              <button
                class="flex h-7 max-w-[260px] cursor-pointer items-center gap-1.5 rounded-md px-2 text-[12px] text-neutral-600 transition-colors hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-40"
                :class="modelDropOpen && 'bg-neutral-100'"
                type="button"
                :disabled="modelLoading || modelOptions.length === 0"
                @click="modelDropOpen = !modelDropOpen"
              >
                <span class="shrink-0 text-neutral-400">+</span>
                <span class="min-w-0 truncate">{{ modelLoading ? '加载中…' : currentModelLabel }}</span>
                <ChevronDown class="h-3 w-3 shrink-0 text-neutral-400" />
              </button>
              <div
                v-if="modelDropOpen"
                class="absolute bottom-full z-10 mb-1 max-h-64 w-[260px] overflow-y-auto rounded-md border border-neutral-200 bg-white py-1 shadow-sm"
              >
                <button
                  v-for="m in modelOptions"
                  :key="m.value"
                  class="flex w-full cursor-pointer items-center gap-2 px-2.5 py-1.5 text-left transition-colors hover:bg-neutral-100"
                  :class="modelId === m.value && 'bg-blue-50/60'"
                  :title="m.description ?? m.value"
                  type="button"
                  @click="chooseModel(m.value)"
                >
                  <span class="min-w-0 flex-1 truncate text-[12px] text-neutral-700">
                    {{ m.name ?? m.value }}
                  </span>
                </button>
              </div>
            </div>
            <!-- 思考程度：引擎探测到 category=effort 才出现（opencode 等无此项则整块隐藏） -->
            <div v-if="effortOptions.length > 0" class="relative" data-dropdown @mousedown.stop>
              <button
                class="flex h-7 cursor-pointer items-center gap-1.5 rounded-md px-2 text-[12px] text-neutral-600 transition-colors hover:bg-neutral-100"
                :class="effortDropOpen && 'bg-neutral-100'"
                type="button"
                @click="effortDropOpen = !effortDropOpen"
              >
                <span class="min-w-0 truncate">
                  {{ effortOptions.find((o) => o.value === effortValue)?.name ?? effortValue ?? '思考程度' }}
                </span>
                <ChevronDown class="h-3 w-3 shrink-0 text-neutral-400" />
              </button>
              <div
                v-if="effortDropOpen"
                class="absolute bottom-full z-10 mb-1 max-h-64 w-[160px] overflow-y-auto rounded-md border border-neutral-200 bg-white py-1 shadow-sm"
              >
                <button
                  v-for="o in effortOptions"
                  :key="o.value"
                  class="flex w-full cursor-pointer items-center gap-2 px-2.5 py-1.5 text-left transition-colors hover:bg-neutral-100"
                  :class="effortValue === o.value && 'bg-blue-50/60'"
                  :title="o.description ?? o.value"
                  type="button"
                  @click="chooseEffort(o.value)"
                >
                  <span class="min-w-0 flex-1 truncate text-[12px] text-neutral-700">
                    {{ o.name ?? o.value }}
                  </span>
                </button>
              </div>
            </div>
            <span v-if="modelError" class="text-[11px] text-neutral-400">{{ modelError }}</span>
            <span v-else-if="!modelLoading && modelOptions.length === 0" class="text-[11px] text-neutral-400">
              默认模型
            </span>
          </div>
        </div>

        <!-- 目标：项目（锁定时只显示，不可切换） -->
        <div class="mt-4">
          <div class="mb-1.5 text-[11px] font-medium tracking-wide text-neutral-400 uppercase">目标</div>
          <div class="relative inline-block" data-dropdown @mousedown.stop>
            <button
              class="flex h-7 cursor-pointer items-center gap-1.5 rounded-full border border-neutral-200 bg-white px-2.5 text-[12px] text-neutral-700 transition-colors hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-60"
              type="button"
              :disabled="projectLocked || projectStore.projects.length === 0"
              @click="projectDropOpen = !projectDropOpen"
            >
              <FolderOpen class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
              <span class="max-w-[200px] truncate">{{ projectName }}</span>
              <ChevronDown v-if="!projectLocked" class="h-3 w-3 shrink-0 text-neutral-400" />
            </button>
            <div
              v-if="projectDropOpen"
              class="absolute z-10 mt-1 max-h-60 w-[240px] overflow-y-auto rounded-md border border-neutral-200 bg-white py-1 shadow-sm"
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
          <p v-else class="mt-1.5 text-[11px] text-neutral-400">
            <template v-if="!editing && projectLocked">任务将创建在当前项目中。</template>
            <template v-else>任务将在该文件夹中创建会话并执行。</template>
          </p>
        </div>
      </div>

      <!-- 页脚 -->
      <div class="flex shrink-0 items-center justify-end gap-2 border-t border-neutral-200 px-5 py-3">
        <span class="mr-auto text-[11px] text-neutral-400">Ctrl / ⌘ + Enter 保存</span>
        <button
          class="cursor-pointer rounded-md px-3 py-1.5 text-[13px] text-neutral-600 transition-colors hover:bg-neutral-100"
          @click="emit('close')"
        >
          取消
        </button>
        <button
          class="cursor-pointer rounded-md bg-neutral-900 px-3 py-1.5 text-[13px] text-white transition-colors hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="!canSubmit"
          @click="submit"
        >
          {{ editing ? '保存' : '创建' }}
        </button>
      </div>
    </div>
  </div>
</template>
