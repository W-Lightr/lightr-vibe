<script setup lang="ts">
// 新建会话弹窗：选择引擎 + 目标文件夹后创建会话（PRD R2）。
// 列表来自 engine store（经 acp_list_agents 下发，前端不写死）；预选 = 默认引擎，
// 默认引擎不可用时 fallback 首个 available（照 codeg AgentSelector：fallback 不写回默认）。
// 文件夹来源 = 已打开的项目列表（projectStore.projects）；不支持在弹窗内新建文件夹。
import { computed, onMounted, ref } from 'vue'
import { ChevronDown, Folder, X } from 'lucide-vue-next'
import { engineIcon } from '../../../utils/engineIcons'
import { useEngineStore } from '../../../stores/engine'
import { useProjectStore } from '../../../stores/project'
import type { EngineWire } from '../../../types/engine'
import type { NewSessionTarget } from '../../../types/session'

const props = defineProps<{
  /** 弹窗是否可见（父级 v-model 控制） */
  open: boolean
  /** 初始选中的项目 id（父级可预置；仅用于初始选中） */
  defaultProjectId?: string | null
  /** 仅选引擎模式（文件夹已由调用方确定，如侧栏文件夹行内「新建会话」）：
   * 隐藏「选择文件夹」，目标固定为该文件夹；确认后由父级按 defaultProjectId 建会话。 */
  engineOnly?: boolean
}>()

const emit = defineEmits<{
  close: []
  /** 用户确认：携带所选引擎 wire 与目标（文件夹 / 临时会话） */
  confirm: [{ engineId: EngineWire; target: NewSessionTarget }]
}>()

const engineStore = useEngineStore()
const projectStore = useProjectStore()

/** 本次引擎选择（默认预选；打开时同步为 preselectedEngineId） */
const selected = ref<EngineWire>(engineStore.preselectedEngineId)

/** 本次目标项目 id（null = 临时会话，不选文件夹） */
const selectedProjectId = ref<string | null>(null)

// 弹窗打开时同步预选并拉取最新列表（失败保留缓存）
onMounted(() => {
  void engineStore.load()
})

/** 打开时同步预选：引擎 + 文件夹（默认 = 父级预置 > 最近项目 > 临时会话）。
 * engineOnly 模式下文件夹锁定为 defaultProjectId（不可选）。 */
function syncOnOpen() {
  selected.value = engineStore.preselectedEngineId
  selectedProjectId.value = props.engineOnly
    ? (props.defaultProjectId ?? null)
    : (props.defaultProjectId ?? projectStore.recentProjects[0]?.id ?? null)
}

const list = computed(() => engineStore.agents)

/** 当前选中引擎条目（收起态展示图标 + 名称） */
const selectedAgent = computed(
  () => list.value.find((x) => x.agentType === selected.value) ?? null,
)

/** 文件夹下拉选项：临时会话（null）+ 已有项目 */
const projectOptions = computed(() => projectStore.projects)

/** 当前选中项目名（无 → 临时会话） */
const selectedProjectName = computed(
  () => projectStore.get(selectedProjectId.value)?.name ?? '临时会话（不选文件夹）',
)

/** 无已有文件夹：只能选临时会话（PRD AC8） */
const hasProjects = computed(() => projectStore.projects.length > 0)

/** 下拉展开态（引擎 / 文件夹各自独立） */
const dropOpen = ref(false)
const projectDropOpen = ref(false)

function choose(agentType: EngineWire) {
  selected.value = agentType
  dropOpen.value = false
}

function chooseProject(projectId: string | null) {
  selectedProjectId.value = projectId
  projectDropOpen.value = false
}

/** 点击弹窗其它区域时收起下拉 */
function onRootMousedown() {
  dropOpen.value = false
  projectDropOpen.value = false
}

const canConfirm = computed(() => {
  const a = list.value.find((x) => x.agentType === selected.value)
  return !!a && a.enabled
})

function confirm() {
  if (!canConfirm.value) return
  engineStore.pickEngine(selected.value)
  const target: NewSessionTarget = props.engineOnly
    ? { kind: 'folder', projectId: props.defaultProjectId as string }
    : selectedProjectId.value
      ? { kind: 'folder', projectId: selectedProjectId.value }
      : { kind: 'tmp' }
  emit('confirm', { engineId: selected.value, target })
  emit('close')
}

function close() {
  emit('close')
}
</script>

<template>
  <div
    v-if="props.open"
    data-no-drag
    class="fixed inset-0 z-50 flex items-center justify-center bg-neutral-900/20"
    @mousedown.self="close"
  >
    <div
      class="w-[340px] rounded-2xl border border-neutral-200 bg-white p-5 shadow-lg"
      @mousedown.stop="onRootMousedown"
      @vue:mounted="syncOnOpen"
    >
      <div class="mb-3 flex items-center justify-between">
        <span class="text-[13px] font-medium text-neutral-800">新建会话</span>
        <button
          class="flex h-6 w-6 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
          title="关闭"
          @click="close"
        >
          <X class="h-3.5 w-3.5" />
        </button>
      </div>

      <div class="mb-1.5 text-[11px] text-neutral-500">选择引擎</div>
      <div class="relative" @mousedown.stop>
        <!-- 收起态：当前引擎（图标 + 名称） -->
        <button
          class="flex w-full cursor-pointer items-center gap-2 rounded-lg border border-neutral-300 bg-white px-2.5 py-2 text-left transition-colors hover:bg-neutral-50"
          :class="dropOpen && 'border-blue-400'"
          type="button"
          @click="dropOpen = !dropOpen"
        >
          <span
            class="h-4 w-4 shrink-0 text-neutral-600 [&>svg]:h-4 [&>svg]:w-4"
            v-html="engineIcon(selectedAgent?.agentType ?? '')"
          />
          <span class="min-w-0 flex-1 truncate text-[13px] text-neutral-800">
            {{ selectedAgent?.name ?? '选择引擎' }}
          </span>
          <ChevronDown
            class="h-3.5 w-3.5 shrink-0 text-neutral-400 transition-transform"
            :class="dropOpen && 'rotate-180'"
          />
        </button>

        <!-- 展开态：引擎列表（每项图标 + 名称 + 描述） -->
        <div
          v-if="dropOpen"
          class="absolute z-10 mt-1 max-h-60 w-full overflow-y-auto rounded-lg border border-neutral-200 bg-white py-1 shadow-sm"
        >
          <button
            v-for="agent in list"
            :key="agent.agentType"
            class="flex w-full cursor-pointer flex-col items-start gap-0.5 px-2.5 py-2 text-left transition-colors hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-40"
            :class="selected === agent.agentType && 'bg-blue-50/60'"
            :disabled="!agent.enabled"
            type="button"
            @click="choose(agent.agentType)"
          >
            <div class="flex w-full items-center gap-2">
              <span
                class="h-4 w-4 shrink-0 text-neutral-600 [&>svg]:h-4 [&>svg]:w-4"
                v-html="engineIcon(agent.agentType)"
              />
              <span class="min-w-0 flex-1 truncate text-[13px] text-neutral-800">{{ agent.name }}</span>
              <span v-if="!agent.available" class="shrink-0 text-[10px] text-amber-500">未安装</span>
            </div>
            <span v-if="agent.description" class="pl-6 text-[11px] text-neutral-500">
              {{ agent.description }}
            </span>
          </button>
        </div>
      </div>

      <!-- engineOnly：文件夹已由调用方确定，显示只读提示；否则给下拉选择 -->
      <template v-if="props.engineOnly">
        <div class="mt-3 mb-1.5 text-[11px] text-neutral-500">目标文件夹</div>
        <div class="flex w-full items-center gap-2 rounded-lg border border-neutral-200 bg-neutral-50 px-2.5 py-2">
          <Folder class="h-4 w-4 shrink-0 text-neutral-500" />
          <span class="min-w-0 flex-1 truncate text-[13px] text-neutral-600">
            {{ projectStore.get(props.defaultProjectId)?.name ?? '临时会话' }}
          </span>
        </div>
      </template>
      <template v-else>
      <div class="mt-3 mb-1.5 text-[11px] text-neutral-500">选择文件夹</div>
      <div class="relative" @mousedown.stop>
        <!-- 收起态：当前目标（文件夹 / 临时会话） -->
        <button
          class="flex w-full cursor-pointer items-center gap-2 rounded-lg border border-neutral-300 bg-white px-2.5 py-2 text-left transition-colors hover:bg-neutral-50"
          :class="projectDropOpen && 'border-blue-400'"
          type="button"
          :disabled="!hasProjects"
          @click="projectDropOpen = !projectDropOpen"
        >
          <Folder class="h-4 w-4 shrink-0 text-neutral-500" />
          <span class="min-w-0 flex-1 truncate text-[13px] text-neutral-800">
            {{ selectedProjectName }}
          </span>
          <ChevronDown
            class="h-3.5 w-3.5 shrink-0 text-neutral-400 transition-transform"
            :class="projectDropOpen && 'rotate-180'"
          />
        </button>

        <!-- 展开态：临时会话 + 已有文件夹列表 -->
        <div
          v-if="projectDropOpen"
          class="absolute z-10 mt-1 max-h-60 w-full overflow-y-auto rounded-lg border border-neutral-200 bg-white py-1 shadow-sm"
        >
          <button
            class="flex w-full cursor-pointer items-center gap-2 px-2.5 py-2 text-left transition-colors hover:bg-neutral-100"
            :class="selectedProjectId === null && 'bg-blue-50/60'"
            type="button"
            @click="chooseProject(null)"
          >
            <span class="min-w-0 flex-1 truncate text-[13px] text-neutral-800">
              临时会话（不选文件夹）
            </span>
          </button>
          <button
            v-for="p in projectOptions"
            :key="p.id"
            class="flex w-full cursor-pointer items-center gap-2 px-2.5 py-2 text-left transition-colors hover:bg-neutral-100"
            :class="selectedProjectId === p.id && 'bg-blue-50/60'"
            :title="p.rootPath"
            type="button"
            @click="chooseProject(p.id)"
          >
            <Folder class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
            <span class="min-w-0 flex-1 truncate text-[13px] text-neutral-800">{{ p.name }}</span>
          </button>
        </div>
      </div>
      <!-- 无已有文件夹：提示先打开一个文件夹（PRD AC8） -->
      <p v-if="!hasProjects" class="mt-1.5 text-[11px] text-neutral-400">
        暂无已打开的文件夹，请先打开一个文件夹，或使用临时会话
      </p>
      </template>

      <div class="mt-4 flex justify-end gap-2">
        <button
          class="cursor-pointer rounded-lg border border-neutral-200 px-3 py-1.5 text-[13px] text-neutral-600 transition-colors hover:bg-neutral-100"
          @click="close"
        >
          取消
        </button>
        <button
          class="cursor-pointer rounded-lg bg-blue-500 px-3 py-1.5 text-[13px] text-white transition-colors hover:bg-blue-600 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="!canConfirm"
          @click="confirm"
        >
          创建
        </button>
      </div>
    </div>
  </div>
</template>
