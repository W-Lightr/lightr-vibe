<script setup lang="ts">
// 自动化编辑器（对齐设计图 2）：纵向表单，无分组卡片边框。
//
// 结构：← 模板返回 / 名称（大号占位）/ 引擎胶囊行 / 提示词大框 /
//       动作（分段）/ 运行目标（项目选择 + worktree 勾选）/
//       触发方式（分段）→ 计划条 / 取消·保存
//
// 校验：前端只做提示，权威在 Rust `validate_draft`（保存失败的串直接显示）。
// 「下次运行」一律调 automation_compute_next_run —— 与调度器同源（design §4.4）。
import { computed, ref, watch } from 'vue'
import { Check, FolderClosed, LoaderCircle, Wand2 } from 'lucide-vue-next'
import { engineIcon } from '../../../utils/engineIcons'
import { formatDateTime } from '../../../utils/time'
import { automationService } from '../../../services/automationService'
import { useEngineStore } from '../../../stores/engine'
import { useProjectStore } from '../../../stores/project'
import { automationErrorMessage } from '../../../types/automation'
import type {
  Automation,
  AutomationAction,
  AutomationDraft,
  NextRunPreview,
  TriggerKind,
} from '../../../types/automation'
import type { EngineWire } from '../../../types/engine'
import { CRON_PRESETS, scheduleLabel } from '../scheduleLabel'
import CronBuilderDialog from './CronBuilderDialog.vue'

const props = defineProps<{
  open: boolean
  /** 编辑的自动化（null = 新建） */
  automation: Automation | null
}>()

const emit = defineEmits<{
  close: []
  saved: [automation: Automation]
}>()

const projectStore = useProjectStore()
const engineStore = useEngineStore()

const name = ref('')
const prompt = ref('')
const projectId = ref<string | null>(null)
const engineId = ref<EngineWire>(engineStore.preselectedEngineId)
const action = ref<AutomationAction>('launch_session')
const triggerKind = ref<TriggerKind>('schedule')
const cron = ref('0 9 * * 1-5')
const timezone = ref('Asia/Shanghai')
const enabled = ref(true)
const useWorktree = ref(true)
const baseBranch = ref('')
const initCommand = ref('')

const saving = ref(false)
const errorText = ref<string | null>(null)
const cronDialogOpen = ref(false)
const preview = ref<NextRunPreview | null>(null)
let previewSeq = 0

/** 可选引擎（只列已装好的，避免选了跑不起来） */
const catalog = computed(() => engineStore.agents.filter((a) => a.available))

const project = computed(() => projectStore.get(projectId.value))

/** 计划条的胶囊：预设 + 当前自定义表达式（去重） */
const quickCrons = computed(() => {
  const presets = CRON_PRESETS.filter((p) =>
    ['每小时整点', '每天 09:00', '工作日 09:00'].includes(p.label),
  )
  return presets
})

const planSummary = computed(() =>
  triggerKind.value === 'manual'
    ? '仅手动触发'
    : scheduleLabel(cron.value, '').replace(/ · .*$/, ''),
)

const planNextRun = computed(() => {
  if (triggerKind.value === 'manual' || !enabled.value) return null
  const at = preview.value?.nextRunAt
  return at ? formatDateTime(at) : null
})

const timezoneHint = computed(() => {
  const p = preview.value
  if (!p || triggerKind.value === 'manual') return null
  if (!p.timezoneSupported) return '该时区暂不支持，将按 UTC 求值'
  if (p.timezoneHasDst) return '该时区有夏令时，本版本按标准时偏移求值'
  return null
})

watch(
  () => props.open,
  (open) => {
    if (!open) return
    const a = props.automation
    name.value = a?.name ?? ''
    prompt.value = a?.prompt ?? ''
    projectId.value = a?.projectId ?? (projectStore.projects[0]?.id ?? null)
    engineId.value = a?.engineId ?? engineStore.preselectedEngineId
    action.value = a?.action ?? 'launch_session'
    triggerKind.value = a?.triggerKind ?? 'schedule'
    cron.value = a?.cron || '0 9 * * 1-5'
    timezone.value = a?.timezone || 'Asia/Shanghai'
    enabled.value = a?.enabled ?? true
    useWorktree.value = a?.useWorktree ?? true
    baseBranch.value = a?.baseBranch ?? ''
    initCommand.value = a?.initCommand ?? ''
    errorText.value = null
    preview.value = null
    schedulePreview()
  },
)

let previewTimer: ReturnType<typeof setTimeout> | null = null
function schedulePreview() {
  if (previewTimer) clearTimeout(previewTimer)
  if (triggerKind.value === 'manual') {
    preview.value = null
    return
  }
  previewTimer = setTimeout(() => {
    void refreshPreview()
  }, 200)
}

async function refreshPreview() {
  const seq = ++previewSeq
  try {
    const result = await automationService.computeNextRun(cron.value, timezone.value)
    if (seq === previewSeq) preview.value = result
  } catch {
    if (seq === previewSeq) preview.value = null
  }
}

watch([cron, timezone, triggerKind], schedulePreview)

function onCronConfirm(value: string) {
  cron.value = value
  cronDialogOpen.value = false
}

function buildDraft(): AutomationDraft | null {
  const p = project.value
  if (!p) {
    errorText.value = projectStore.projects.length === 0
      ? '还没有打开过任何项目，请先在左侧打开一个文件夹'
      : '请选择一个运行目标'
    return null
  }
  return {
    name: name.value,
    enabled: enabled.value,
    triggerKind: triggerKind.value,
    cron: triggerKind.value === 'schedule' ? cron.value.trim() : '',
    timezone: timezone.value.trim(),
    engineId: engineId.value,
    projectId: p.id,
    rootPath: p.rootPath,
    modelId: null,
    effortId: null,
    effortValue: null,
    prompt: prompt.value,
    action: action.value,
    useWorktree: useWorktree.value,
    baseBranch: useWorktree.value ? baseBranch.value.trim() || null : null,
    initCommand: useWorktree.value ? initCommand.value.trim() || null : null,
  }
}

async function submit() {
  if (saving.value) return
  errorText.value = null
  const draft = buildDraft()
  if (!draft) return
  saving.value = true
  try {
    const saved = props.automation
      ? await automationService.update(props.automation.id, draft)
      : await automationService.create(draft)
    emit('saved', saved)
  } catch (err) {
    errorText.value = automationErrorMessage(err)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div
    v-if="props.open"
    data-no-drag
    class="absolute inset-0 z-50 flex items-start justify-center overflow-y-auto bg-white/95 backdrop-blur-sm"
    @mousedown.self="emit('close')"
  >
    <div class="w-full max-w-[640px] px-8 pt-5 pb-8">
      <!-- 返回 -->
      <button
        class="flex cursor-pointer items-center gap-1.5 text-[11px] text-neutral-400 transition-colors hover:text-neutral-600"
        @click="emit('close')"
      >
        <span class="text-[13px] leading-none">←</span>
        模板
      </button>

      <p v-if="errorText" class="mt-4 rounded-md bg-rose-50 px-3 py-2 text-[11px] text-rose-700">
        {{ errorText }}
      </p>

      <!-- 名称 -->
      <input
        v-model="name"
        class="mt-3 w-full border-0 bg-transparent p-0 text-[19px] font-normal text-neutral-900 outline-none placeholder:text-neutral-300"
        placeholder="例如：每晚 PR 审查"
        maxlength="120"
      />

      <!-- 引擎胶囊行 -->
      <div class="mt-3 flex flex-wrap items-center gap-1">
        <span
          v-for="agent in catalog"
          :key="agent.agentType"
          class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-full border transition-colors"
          :class="
            engineId === agent.agentType
              ? 'border-neutral-300 bg-white shadow-[0_1px_2px_rgba(0,0,0,0.06)]'
              : 'border-transparent hover:bg-neutral-100'
          "
          :title="agent.name"
          @click="engineId = agent.agentType"
        >
          <span
            class="h-4 w-4 text-neutral-700 [&>svg]:h-4 [&>svg]:w-4"
            v-html="engineIcon(agent.agentType)"
          />
        </span>
        <span class="ml-1 text-[11px] text-neutral-400">
          {{ catalog.find((a) => a.agentType === engineId)?.name ?? '选择智能体' }}
        </span>
      </div>

      <!-- 提示词 -->
      <div class="mt-3 rounded-xl border border-neutral-200 px-3.5 py-2.5 focus-within:border-neutral-400">
        <textarea
          v-model="prompt"
          rows="5"
          class="block w-full resize-none border-0 bg-transparent p-0 text-[13px] leading-5 text-neutral-800 outline-none placeholder:text-neutral-400"
          placeholder="让智能体做什么？"
        />
        <div class="mt-1.5 flex items-center gap-1.5 text-[11px] text-neutral-400">
          <LoaderCircle class="h-3 w-3" />
          每次触发时原样发送
        </div>
      </div>

      <!-- 动作 -->
      <p class="mt-3.5 text-[11px] text-neutral-400">动作</p>
      <div class="mt-1.5 flex gap-1">
        <button
          v-for="key in (['launch_session', 'enqueue_task'] as AutomationAction[])"
          :key="key"
          class="h-7 cursor-pointer rounded-lg px-3 text-[12px] transition-colors"
          :class="
            action === key
              ? 'bg-neutral-900 text-white'
              : 'border border-neutral-200 text-neutral-600 hover:bg-neutral-50'
          "
          @click="action = key"
        >
          {{ key === 'launch_session' ? '启动会话' : '任务入队' }}
        </button>
      </div>
      <p class="mt-1.5 text-[10px] leading-4 text-neutral-400">
        {{
          action === 'launch_session'
            ? '直接拉起后台会话把提示词跑完（会自动允许工具调用），结果记在运行历史里。'
            : '只在任务看板上建一张待办卡，由你自己决定什么时候启动。'
        }}
      </p>

      <!-- 运行目标 -->
      <p class="mt-3.5 text-[11px] text-neutral-400">运行目标</p>
      <div class="mt-1.5 flex flex-wrap items-center gap-3">
        <div
          class="flex h-8 items-center gap-1.5 rounded-lg border border-neutral-200 px-2.5 text-[12px]"
        >
          <FolderClosed class="h-3.5 w-3.5 text-neutral-400" />
          <select
            v-model="projectId"
            class="max-w-[180px] cursor-pointer border-0 bg-transparent text-[12px] text-neutral-700 outline-none"
          >
            <option :value="null" disabled>选择项目</option>
            <option v-for="p in projectStore.projects" :key="p.id" :value="p.id">
              {{ p.name }}
            </option>
          </select>
        </div>

        <label class="flex cursor-pointer items-center gap-1.5 text-[12px] text-neutral-600">
          <input v-model="useWorktree" type="checkbox" class="accent-neutral-900" />
          每次运行新建 worktree
        </label>
      </div>
      <p v-if="project" class="mt-1.5 truncate font-mono text-[10px] text-neutral-400">
        {{ project.rootPath }}
      </p>

      <template v-if="useWorktree">
        <input
          v-model="baseBranch"
          class="mt-1.5 w-full rounded-lg border border-neutral-200 px-2.5 py-1 text-[11px] outline-none focus:border-neutral-400"
          placeholder="基线分支（留空 = 仓库当前分支）"
          spellcheck="false"
        />
        <input
          v-model="initCommand"
          class="mt-1.5 w-full rounded-lg border border-neutral-200 px-2.5 py-1 text-[11px] outline-none focus:border-neutral-400"
          placeholder="初始化命令（可选，如 pnpm install）"
          spellcheck="false"
        />
      </template>

      <!-- 触发方式 -->
      <p class="mt-3.5 text-[11px] text-neutral-400">触发方式</p>
      <div class="mt-1.5 flex gap-1">
        <button
          class="h-7 cursor-pointer rounded-lg px-3 text-[12px] transition-colors"
          :class="
            triggerKind === 'schedule'
              ? 'bg-neutral-900 text-white'
              : 'border border-neutral-200 text-neutral-600 hover:bg-neutral-50'
          "
          @click="triggerKind = 'schedule'"
        >
          按计划
        </button>
        <button
          class="h-7 cursor-pointer rounded-lg px-3 text-[12px] transition-colors"
          :class="
            triggerKind === 'manual'
              ? 'bg-neutral-900 text-white'
              : 'border border-neutral-200 text-neutral-600 hover:bg-neutral-50'
          "
          @click="triggerKind = 'manual'"
        >
          仅手动
        </button>
      </div>

      <!-- 计划条 -->
      <div
        v-if="triggerKind === 'schedule'"
        class="mt-2 rounded-xl border border-neutral-200 px-3 py-2.5"
      >
        <!-- 快捷预设 -->
        <div class="flex flex-wrap items-center gap-1.5">
          <button
            v-for="preset in quickCrons"
            :key="preset.cron"
            class="h-7 cursor-pointer rounded-full px-2.5 text-[11px] transition-colors"
            :class="
              cron.trim() === preset.cron
                ? 'bg-neutral-900 text-white'
                : 'border border-neutral-200 text-neutral-600 hover:bg-neutral-50'
            "
            @click="cron = preset.cron"
          >
            {{ preset.label.replace(' 09:00', ' 9 点') }}
          </button>
          <button
            class="h-7 cursor-pointer rounded-full px-2.5 text-[11px] transition-colors"
            :class="
              !quickCrons.some((p) => p.cron === cron.trim())
                ? 'bg-neutral-900 text-white'
                : 'border border-neutral-200 text-neutral-600 hover:bg-neutral-50'
            "
            @click="cronDialogOpen = true"
          >
            自定义
          </button>
        </div>

        <!-- 表达式 + 魔术棒 -->
        <div
          class="mt-2 flex h-8 items-center gap-2 rounded-lg border border-neutral-300 px-2.5 focus-within:border-neutral-500"
        >
          <input
            v-model="cron"
            class="min-w-0 flex-1 border-0 bg-transparent font-mono text-[12px] text-neutral-800 outline-none"
            placeholder="0 9 * * 1-5"
            spellcheck="false"
          />
          <button
            class="flex h-5 w-5 shrink-0 cursor-pointer items-center justify-center rounded text-neutral-400 hover:bg-neutral-100 hover:text-neutral-700"
            title="用计划生成器"
            @click="cronDialogOpen = true"
          >
            <Wand2 class="h-3.5 w-3.5" />
          </button>
        </div>

        <!-- 计划摘要 + 下次运行 + 时区 -->
        <p class="mt-2 text-[11px] text-neutral-700">{{ planSummary }}</p>
        <div class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px]">
          <span v-if="planNextRun" class="text-neutral-500">下次运行：{{ planNextRun }}</span>
          <span v-else-if="preview" class="text-rose-600">该表达式不会再触发</span>
          <span class="flex items-center gap-1 text-neutral-400">
            <span class="text-[10px]">🌐</span>
            <input
              v-model="timezone"
              class="w-32 border-0 bg-transparent text-[11px] text-neutral-500 outline-none"
              spellcheck="false"
            />
          </span>
        </div>
        <p v-if="timezoneHint" class="mt-1 text-[10px] text-amber-600">{{ timezoneHint }}</p>
      </div>

      <p v-else class="mt-2 text-[11px] text-neutral-400">
        保存后不会自动运行，只能从卡片上手动点「运行」。
      </p>

      <!-- 启用 -->
      <label class="mt-3 flex cursor-pointer items-center gap-1.5 text-[11px] text-neutral-500">
        <input v-model="enabled" type="checkbox" class="accent-neutral-900" />
        保存后立即启用
      </label>

      <!-- 底部操作 -->
      <div class="mt-5 flex items-center justify-end gap-2">
        <button
          class="h-9 cursor-pointer px-3 text-[12px] text-neutral-500 hover:text-neutral-800"
          @click="emit('close')"
        >
          取消
        </button>
        <button
          class="flex h-8 cursor-pointer items-center gap-1.5 rounded-full bg-neutral-900 px-4 text-[12px] text-white hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="saving"
          @click="submit"
        >
          <Check v-if="!saving" class="h-3.5 w-3.5" />
          <LoaderCircle v-else class="h-3.5 w-3.5 animate-spin" />
          {{ saving ? '保存中' : '保存' }}
        </button>
      </div>
    </div>

    <CronBuilderDialog
      :open="cronDialogOpen"
      :cron="cron"
      :timezone="timezone"
      :preview="preview"
      @close="cronDialogOpen = false"
      @confirm="onCronConfirm"
    />
  </div>
</template>
