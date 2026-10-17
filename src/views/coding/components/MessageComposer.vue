<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { NPopover, NSelect, useMessage } from 'naive-ui'
import {
  Plus,
  ArrowUp,
  Square,
  Paperclip,
  X,
  Shield,
  ChevronDown,
  ChevronRight,
  ChevronLeft,
  Search,
  Check,
  Cpu,
  Gauge,
  Folder,
  GitBranch,
  RefreshCw,
  HeartHandshake,
  HeartCrack,
  HeartOff,
  HeartPulse,
} from 'lucide-vue-next'
import { MODELS, EXECUTION_MODES, useWorkspaceStore } from '../../../stores/workspace'
import { useSessionStore } from '../../../stores/session'
import { useEngineStore } from '../../../stores/engine'
import { useProjectStore } from '../../../stores/project'
import { useAcpPermission } from '../../../composables/useAcpPermission'
import { useAcpQuestion } from '../../../composables/useAcpQuestion'
import PermissionCard from './PermissionCard.vue'
import AskQuestionCard from './AskQuestionCard.vue'
import {
  useConversation,
  ensureModelOptions,
  refreshModelOptions,
  useAcpHealth,
  getDraftInput,
  setDraftInput,
  clearDraftInput,
  configRejectedNotice,
  sessionNoticeState,
} from '../../../composables/useConversation'
import type { AcpConfigOption, AcpConfigOptionGroup, AcpConfigOptionValue } from '../../../types/acp'
import { formatTokens } from '../../../utils/time'

const props = defineProps<{ sessionId: string }>()

const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
const engineStore = useEngineStore()
const projectStore = useProjectStore()

// 权限审批卡（贴在输入框上沿，与输入框融合为一体）：按 connId 过滤本会话待审批
const { pending: pendingPermissions } = useAcpPermission()
const permissionQueue = computed(() =>
  pendingPermissions.value.filter((e) => e.connId === props.sessionId),
)
// elicitation 提问卡（同区域、浮在输入框上方）：按 connId 过滤本会话待答（本期一次一张）
const { pending: pendingQuestions } = useAcpQuestion()
const questionQueue = computed(() =>
  pendingQuestions.value.filter((e) => e.connId === props.sessionId),
)
const { send, stop, setModel, setConfigOption, reconnect, reconnecting, ensureConnected, closeConnection, resolveContextWindow } =
  useConversation()
const { healthOf, acpConnectingSessionIds, acpLiveSessionIds, refreshAcpStatus } = useAcpHealth()
const message = useMessage()

// 草稿输入从模块级暂存恢复（跨会话切换保留未发送文字，延迟入列 PRD R5）
const draft = ref(getDraftInput(props.sessionId))
const attachments = ref<string[]>([])
const textareaEl = ref<HTMLTextAreaElement | null>(null)
const fileInputEl = ref<HTMLInputElement | null>(null)

const generating = computed(() => workspace.isGenerating(props.sessionId))
const canSend = computed(() => draft.value.trim().length > 0 && !generating.value)
const session = computed(() => sessionStore.get(props.sessionId))

/** 模型配置项（session/new configOptions）——无则用 MODELS 静态兜底 */
const modelConfig = computed<AcpConfigOption | null>(() => {
  return session.value?.configOptions?.find((o) => o.id === 'model' || o.category === 'model') ?? null
})

/** 思考强度配置项（仅推理模型暴露 effort，probe §2） */
const effortConfig = computed<AcpConfigOption | null>(() => {
  return session.value?.configOptions?.find((o) => o.id === 'effort' || o.category === 'effort') ?? null
})

interface FlatOption {
  label: string
  value: string
}

/** configOptions.select 展开（扁平与分组两种形态，协议 §8） */
function flattenOptions(config: AcpConfigOption | null): FlatOption[] {
  const flat: FlatOption[] = []
  for (const opt of config?.options ?? []) {
    if ('value' in opt) {
      const v = opt as AcpConfigOptionValue
      flat.push({ label: v.name ?? v.value, value: v.value })
    } else {
      const g = opt as AcpConfigOptionGroup
      for (const sub of g.options ?? []) {
        flat.push({
          label: g.group ? `${g.group} / ${sub.name ?? sub.value}` : (sub.name ?? sub.value),
          value: sub.value,
        })
      }
    }
  }
  return flat
}

/** 模型列表：configOptions 优先，MODELS 静态数组兜底（浏览器 mock / 未建连） */
const modelOptions = computed<FlatOption[]>(() => {
  const flat = flattenOptions(modelConfig.value)
  if (flat.length) return flat
  return MODELS.map((m) => ({ label: m, value: m }))
})

/** 思考强度列表（扁平取 name/value） */
const effortOptions = computed<FlatOption[]>(() => flattenOptions(effortConfig.value))

/** 模型短名：取 value 末段（去掉 provider 前缀），无则原样 */
function shortName(value: string): string {
  const idx = value.lastIndexOf('/')
  return idx >= 0 ? value.slice(idx + 1) : value
}

/** 模型品牌图标：按名称/值关键字匹配 public/logos/<brand>.svg（LobeHub 图标集），
 * 全部未命中返回 null（回退 lucide 通用图标）。关键字顺序即优先级。 */
const LOGO_RULES: { keys: string[]; file: string }[] = [
  { keys: ['claude', 'anthropic'], file: 'anthropic' },
  { keys: ['qwen', 'qwq', 'tongyi', '通义'], file: 'qwen' },
  { keys: ['gemini', 'gemma', 'google', 'gemma'], file: 'google' },
  { keys: ['deepseek'], file: 'deepseek' },
  { keys: ['moonshot', 'kimi'], file: 'moonshotai' },
  { keys: ['mistral', 'mixtral'], file: 'mistralai' },
  { keys: ['grok', 'xai'], file: 'xai' },
  { keys: ['nvidia'], file: 'nvidia' },
  { keys: ['perplexity'], file: 'perplexity' },
  { keys: ['zhipu', 'glm', 'chatglm'], file: 'zhipu' },
  { keys: ['ollama'], file: 'ollama' },
  { keys: ['llama', 'meta'], file: 'meta' },
  { keys: ['gpt', 'openai', 'o1', 'o3', 'o4', 'chatgpt'], file: 'openai' },
]

/** 命中品牌 → `/logos/<file>.svg`；未命中 → null */
function modelLogo(value: string, label?: string): string | null {
  const key = `${value} ${label ?? ''}`.toLowerCase()
  for (const rule of LOGO_RULES) {
    if (rule.keys.some((k) => key.includes(k))) return `/logos/${rule.file}.svg`
  }
  return null
}

/** 当前模型值：会话已建连用 session.model，否则用全局默认 */
const currentModel = computed(() => session.value?.model ?? workspace.selectedModel)
const currentModelLabel = computed(() => {
  const hit = modelOptions.value.find((m) => m.value === currentModel.value)
  return shortName(hit?.label ?? currentModel.value)
})

/** chip 图标：按当前模型名关键字匹配品牌 logo */
const currentModelLogo = computed(() => {
  const hit = modelOptions.value.find((m) => m.value === currentModel.value)
  return modelLogo(currentModel.value, hit?.label)
})

/** 当前思考强度值/标签（来自 effort 配置项 currentValue） */
const currentEffort = computed<string | null>(() => {
  const v = effortConfig.value?.currentValue
  return typeof v === 'string' ? v : null
})
const currentEffortLabel = computed(() => {
  const v = currentEffort.value
  if (v === null) return ''
  return effortOptions.value.find((o) => o.value === v)?.label ?? v
})

// ── 悬浮菜单（chip + 下钻） ─────────────────────────────
const popoverOpen = ref(false)
const panel = ref<'root' | 'model' | 'effort'>('root')
const modelSearch = ref('')

// 上下文用量弹层独立于模型 chip 弹层，互不影响
const usageOpen = ref(false)

/** 过滤后的模型列表（按 value/name 大小写不敏感搜索） */
const filteredModels = computed(() => {
  const q = modelSearch.value.trim().toLowerCase()
  if (!q) return modelOptions.value
  return modelOptions.value.filter(
    (m) => m.value.toLowerCase().includes(q) || m.label.toLowerCase().includes(q),
  )
})

/** 渲染行：预解析每项的品牌 logo 路径（避免模板内重复匹配） */
const modelRows = computed(() =>
  filteredModels.value.map((m) => ({ ...m, logo: modelLogo(m.value, m.label) })),
)

// 悬浮菜单每次打开都复位到根面板：状态复位走 show 变化监听而非 trigger 的
// @click，避免与 NPopover 自身的 click 切换叠加导致重复置真/无法关闭
watch(popoverOpen, (open) => {
  if (!open) return
  panel.value = 'root'
  modelSearch.value = ''
})

/** 选择模型：写 workspace/session 乐观值 + set_config_option('model') */
function selectModel(value: string) {
  workspace.selectedModel = value
  if (session.value) session.value.model = value
  void setModel(props.sessionId, value)
  popoverOpen.value = false
}

/** 选择思考强度：set_config_option(effort) */
function selectEffort(value: string) {
  void setConfigOption(props.sessionId, effortConfig.value?.id ?? 'effort', value)
  popoverOpen.value = false
}

/** 「重新获取模型」进行中（小内联 spinner；探针 8-13s，不阻塞输入） */
const refreshingModels = ref(false)
/** 手动触发真实探针刷新模型列表：成功列表原地更新，失败 toast（不产错误卡） */
async function onRefreshModels() {
  if (refreshingModels.value) return
  refreshingModels.value = true
  try {
    const options = await refreshModelOptions(props.sessionId)
    if (!options || !options.length) message.error('获取模型列表失败')
  } catch {
    message.error('获取模型列表失败')
  } finally {
    refreshingModels.value = false
  }
}

const modeOptions = EXECUTION_MODES.map((m) => ({ label: m.label, value: m.value }))

/** 上下文用量百分比（tokenUsage.total 占 contextWindow 的比例，0 时显示 0%）
 * —— total=usage_update.used（上下文占用）；状态栏环取整、弹层内一位小数 */
const contextPct = computed(() => Math.round(contextPercent.value))

/** 精确上下文占比（弹层展示用，一位小数）：total=上下文占用（usage_update.used） */
const contextPercent = computed(() => {
  const t = session.value?.tokenUsage
  if (!t || !t.contextWindow) return 0
  return Math.min(100, (t.total / t.contextWindow) * 100)
})

/** 弹层 token 明细：input/output/cached* 为最近一次回复的口径（prompt response），
 * 总计 = 该次回复四项之和（不等于上下文占用 total，后者用于上下文环） */
const usageDetail = computed(() => {
  const t = session.value?.tokenUsage
  const input = t?.input ?? 0
  const output = t?.output ?? 0
  const cachedRead = t?.cachedRead ?? 0
  const cachedWrite = t?.cachedWrite ?? 0
  return {
    input,
    output,
    cachedRead,
    cachedWrite,
    total: input + output + cachedRead + cachedWrite,
    used: t?.total ?? 0,
    contextWindow: t?.contextWindow ?? 0,
  }
})

/** 状态栏：当前会话所属项目名（临时会话回退「临时会话」） */
const projectName = computed(() => {
  const p = projectStore.get(session.value?.projectId)
  return p?.name ?? '临时会话'
})

/** 状态栏：当前分支（无则显示「无分支」） */
const branchName = computed(() => session.value?.gitBranch || '无分支')

// ── 连接健康（参考 codeg composer 连接健康芯片；每会话独立进程，design §8.3） ──
/** 当前会话是否已建立 ACP 连接（本地会话 id 在 live 集合内） */
const sessionConnected = computed(() => acpLiveSessionIds.value.includes(props.sessionId))
/** 当前会话是否正在建连/重连在途（每连接独立，不吃其它会话的建连状态，design §8.3） */
const sessionConnecting = computed(() =>
  acpConnectingSessionIds.value.includes(props.sessionId),
)
/** 当前会话的连接健康条目（每会话独立进程，design §8.3） */
const acpHealth = computed(() => healthOf(props.sessionId))
/** 视觉态：connecting=建连中 / connected=已连接 / error=异常 / disconnected=未连接。
 * 每会话独立进程（design §8.3）：acpHealth 反映「当前会话」的连接状态，
 * sessionConnected 即本地会话 id ∈ live，sessionConnecting 仅当本会话在途。
 * 优先级：本会话建连中 > 该会话异常 > 已连 > 未连。 */
const healthState = computed<'connecting' | 'connected' | 'error' | 'disconnected'>(() => {
  if (sessionConnecting.value) return 'connecting'
  if (acpHealth.value?.status === 'dead') return 'error'
  if (sessionConnected.value) return 'connected'
  return 'disconnected'
})
const HEALTH_META = {
  connecting: { icon: HeartPulse, label: '连接中…', cls: 'text-blue-500 animate-pulse' },
  connected: { icon: HeartHandshake, label: '已连接', cls: 'text-green-600' },
  error: { icon: HeartCrack, label: '连接异常', cls: 'text-red-500' },
  disconnected: { icon: HeartOff, label: '未连接', cls: 'text-neutral-300' },
} as const
const healthIcon = computed(() => HEALTH_META[healthState.value].icon)
const healthLabel = computed(() => HEALTH_META[healthState.value].label)
const healthClass = computed(() => HEALTH_META[healthState.value].cls)
/** 连接中：文字用与「正在生成…」同款 shimmer 动效 */
const healthShimmer = computed(() => healthState.value === 'connecting')
/** agent 名（无进程信息时按会话引擎取显示名，不写死 OpenCode，R6） */
const agentName = computed(
  () => acpHealth.value?.agentInfo?.name ?? engineStore.agentName(session.value?.engineId),
)

/** 弹层错误原因（仅异常态显示） */
const healthReason = computed(() =>
  healthState.value === 'error' ? (acpHealth.value?.reason ?? null) : null,
)
const healthOpen = ref(false)

/** 可重试错误状态（session_info_update 的 `_meta`）：显示重试横幅而非失败卡（AC5） */
const retryState = computed(() => session.value?.retryState ?? null)
/** 重试横幅文案：带次数（有则）与错误摘要（有则） */
const retryText = computed(() => {
  const r = retryState.value
  if (!r) return ''
  const attempt = typeof r.attempt === 'number' ? `（第 ${r.attempt} 次）` : ''
  return `正在重试${attempt}${r.message ? `：${r.message}` : '…'}`
})

// 配置被 agent 调整为别的值（Rust emit acp://configRejected）：仅对当前会话轻提示
watch(
  () => configRejectedNotice.value.seq,
  () => {
    const notice = configRejectedNotice.value
    if (!notice.seq || notice.connId !== props.sessionId) return
    message.info('该选项被 agent 调整')
  },
)

// session notice（ACP 1.9 Session Notices）：agent 的网关警告等 → 右上角 toast。
// 只按本会话路由（connId）弹出；**不入 transcript**（applyUpdate 已保证）。
watch(
  () => sessionNoticeState.value.seq,
  () => {
    const notice = sessionNoticeState.value
    if (!notice.seq || notice.connId !== props.sessionId) return
    const text = notice.description ? `${notice.title}\n${notice.description}` : notice.title
    message[notice.level](text, { duration: 6000, closable: true })
  },
)

// 模型项 / 当前模型变化（探针异步返回、打开历史会话）时重解析上下文窗口分母，
// 避免分母长期停在 128k 占位或跨模型不更新。
// 不带 immediate：watch 在 setup 期早于 onMounted 求值，首帧由 onMounted 兜底。
watch(
  () => [modelConfig.value?.id, currentModel.value] as const,
  ([id]) => {
    if (id) resolveContextWindow(props.sessionId)
  },
)

/** 重新连接：杀死该会话进程 + 全新 spawn + load-or-new */
async function onReconnect() {
  await reconnect(props.sessionId)
  healthOpen.value = false
}

function autoResize() {
  const el = textareaEl.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = `${Math.min(el.scrollHeight, 200)}px`
}

// 输入变化写回模块级暂存（草稿有未发送文字时切走可恢复，延迟入列 PRD R5）
watch(draft, () => {
  setDraftInput(props.sessionId, draft.value)
  nextTick(autoResize)
})

// 切换会话时清空草稿与附件，并复位悬浮菜单（组件跨会话复用，避免残留
// 上一次会话的下钻面板/搜索词），但不抢占焦点
watch(
  () => props.sessionId,
  (newId, oldId) => {
    // 延迟入列（PRD R5）：切走时若旧会话仍是草稿且输入为空 → 丢弃草稿 + 杀进程。
    // 先读旧 draft 判空，再切换到新会话输入，避免误判。
    const oldWasEmpty = draft.value.trim() === ''
    if (oldId) {
      setDraftInput(oldId, draft.value)
      if (oldWasEmpty && sessionStore.isDraft(oldId)) {
        sessionStore.discardDraft(oldId)
        void closeConnection(oldId)
        clearDraftInput(oldId)
      }
    }
    draft.value = getDraftInput(newId)
    attachments.value = []
    popoverOpen.value = false
    panel.value = 'root'
    modelSearch.value = ''
    nextTick(autoResize)
    // 后台便利探针：会话无模型项时补齐真实列表（失败静默，不阻塞 UI）
    void ensureModelOptions(props.sessionId)
    // 刷新连接健康（该会话独立连接；切会话时同步一次）
    void refreshAcpStatus(props.sessionId)
    // 打开会话即自动建连（对齐 codeg：标签激活即 connect；best-effort 静默）
    void ensureConnected(props.sessionId)
  },
)

onMounted(() => {
  // 仅在页面初次进入且无其他焦点时自动聚焦
  const active = document.activeElement
  if (!active || active === document.body) textareaEl.value?.focus()
  autoResize()
  // 打开会话即预取真实模型列表（缓存命中则零成本）
  void ensureModelOptions(props.sessionId)
  // 刷新连接健康（打开即有值，不必等事件）
  void refreshAcpStatus(props.sessionId)
  // 打开会话即自动建连（不等发送；连上后 configOptions/模型列表即可用）
  void ensureConnected(props.sessionId)
  // 解析上下文窗口分母（模型项就绪即可算，避免长期停在 128k 占位）
  resolveContextWindow(props.sessionId)
})

function submit() {
  const text = draft.value.trim()
  if (!text || generating.value) return
  draft.value = ''
  attachments.value = []
  nextTick(autoResize)
  void send(props.sessionId, text)
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    submit()
  }
}

function pickFiles(e: Event) {
  const input = e.target as HTMLInputElement
  for (const f of input.files ?? []) attachments.value.push(f.name)
  input.value = ''
}
</script>

<template>
  <div class="shrink-0 px-6 pb-3">
    <div class="mx-auto max-w-3xl">
      <!-- 重试横幅：可重试错误（非失败卡），生成期间显示在输入框上方（AC5） -->
      <div
        v-if="retryState"
        class="mx-3 mb-1.5 flex items-center gap-2 rounded-xl border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-700 sm:mx-6"
      >
        <RefreshCw class="h-3.5 w-3.5 shrink-0 animate-spin" />
        <span class="truncate">{{ retryText }}</span>
      </div>

      <!-- 权限审批面板：独立边框、比输入框窄（左右内缩），浮在输入框上方（有间距）；
           不并入输入框边框。 -->
      <div
        v-if="permissionQueue.length > 0"
        class="mx-3 mb-1.5 overflow-hidden rounded-2xl border border-neutral-200 bg-white shadow-sm sm:mx-6"
      >
        <PermissionCard
          v-for="(req, i) in permissionQueue"
          :key="req.requestId"
          :event="req"
          :queued-count="permissionQueue.length - 1 - i"
          :class="i > 0 && 'border-t border-neutral-100'"
        />
      </div>

      <!-- elicitation 提问卡：与权限卡同区域/层级，独立边框浮在输入框上方 -->
      <div
        v-if="questionQueue.length > 0"
        class="mx-3 mb-1.5 overflow-hidden rounded-2xl border border-neutral-200 bg-white shadow-sm sm:mx-6"
      >
        <AskQuestionCard
          v-for="(req, i) in questionQueue"
          :key="req.requestId"
          :event="req"
          :class="i > 0 && 'border-t border-neutral-100'"
        />
      </div>

      <!-- 输入容器：Codex 风格单卡片布局 -->
      <div
        class="rounded-2xl border border-neutral-200 bg-white shadow-sm transition-colors focus-within:border-neutral-300"
      >
        <!-- 附件 chips -->
        <div v-if="attachments.length" class="flex flex-wrap gap-1.5 px-4 pt-3">
          <span
            v-for="(name, i) in attachments"
            :key="`${name}-${i}`"
            class="flex items-center gap-1 rounded-md border border-neutral-200 bg-neutral-50 px-2 py-0.5 text-xs text-neutral-600"
          >
            <Paperclip class="h-3 w-3 text-neutral-400" />
            <span class="max-w-40 truncate">{{ name }}</span>
            <button
              class="cursor-pointer text-neutral-400 hover:text-neutral-600"
              @click="attachments.splice(i, 1)"
            >
              <X class="h-3 w-3" />
            </button>
          </span>
        </div>

        <!-- 多行输入 -->
        <textarea
          ref="textareaEl"
          v-model="draft"
          rows="1"
          placeholder="与 Agent 协作"
          class="block max-h-[200px] min-h-[52px] w-full resize-none bg-transparent px-4 pt-3.5 pb-1 text-[13.5px] leading-6 text-neutral-800 outline-none placeholder:text-neutral-400"
          @keydown="onKeydown"
        />

        <!-- 底部工具栏 -->
        <div class="flex items-center gap-2 px-3 pb-2.5 pt-1">
          <input ref="fileInputEl" type="file" multiple class="hidden" @change="pickFiles" />
          <!-- 添加附件 -->
          <button
            class="flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-100"
            title="添加附件"
            @click="fileInputEl?.click()"
          >
            <Plus class="h-4 w-4" />
          </button>

          <!-- 执行模式（盾牌 + 下拉） -->
          <div class="flex shrink-0 items-center">
            <Shield class="h-3.5 w-3.5 text-neutral-500" />
            <NSelect
              v-model:value="workspace.executionMode"
              :options="modeOptions"
              size="small"
              :consistent-menu-width="false"
              :bordered="false"
              class="mode-select !w-auto"
            >
              <template #arrow>
                <ChevronDown class="h-3.5 w-3.5 text-neutral-400" />
              </template>
            </NSelect>
          </div>

          <div class="flex-1" />

          <!-- 上下文用量：chip 触发用量明细弹层（不随 generating 禁用，用量随时可看） -->
          <NPopover
            v-model:show="usageOpen"
            trigger="click"
            placement="top-end"
            :show-arrow="false"
            raw
            class="usage-popover"
          >
            <template #trigger>
              <button
                class="flex shrink-0 cursor-pointer items-center gap-1 rounded-md px-1.5 py-1 text-[11px] tabular-nums text-neutral-400 transition-colors hover:bg-neutral-100"
                title="上下文用量"
              >
                <span
                  class="inline-block h-2.5 w-2.5 rounded-full border border-neutral-300"
                  :style="{
                    background: `conic-gradient(#60a5fa ${contextPct}%, transparent ${contextPct}%)`,
                  }"
                />
                CH {{ contextPct }}%
              </button>
            </template>

            <div class="w-[240px] overflow-hidden rounded-lg border border-neutral-200 bg-white p-3 shadow-sm">
              <!-- 上下文窗口 -->
              <div class="flex items-baseline justify-between">
                <span class="text-xs text-neutral-600">上下文窗口</span>
                <span class="font-medium text-neutral-700 tabular-nums">
                  {{ contextPercent.toFixed(1) }}%
                </span>
              </div>
              <div class="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-neutral-200">
                <div
                  class="h-full rounded-full bg-blue-400"
                  :style="{ width: `${contextPercent > 0 ? Math.max(1, contextPercent) : 0}%` }"
                />
              </div>
              <div class="mt-1.5 flex items-center justify-between text-[11px]">
                <span class="text-neutral-400">已用 / 上限</span>
                <span class="text-neutral-500 tabular-nums">
                  {{ formatTokens(usageDetail.used) }} / {{ formatTokens(usageDetail.contextWindow) }}
                </span>
              </div>

              <div class="my-2 h-px bg-neutral-200" />

              <!-- Token 用量 -->
              <div class="mb-1.5 text-xs font-medium text-neutral-600">Token 用量</div>
              <div class="flex flex-col gap-1">
                <div class="flex justify-between text-[11px]">
                  <span class="text-neutral-500">输入</span>
                  <span class="text-neutral-500 tabular-nums">{{ formatTokens(usageDetail.input) }}</span>
                </div>
                <div class="flex justify-between text-[11px]">
                  <span class="text-neutral-500">输出</span>
                  <span class="text-neutral-500 tabular-nums">{{ formatTokens(usageDetail.output) }}</span>
                </div>
                <div class="flex justify-between text-[11px]">
                  <span class="text-neutral-500">缓存读取</span>
                  <span class="text-neutral-500 tabular-nums">{{ formatTokens(usageDetail.cachedRead) }}</span>
                </div>
                <div class="flex justify-between text-[11px]">
                  <span class="text-neutral-500">缓存写入</span>
                  <span class="text-neutral-500 tabular-nums">{{ formatTokens(usageDetail.cachedWrite) }}</span>
                </div>
                <div class="flex justify-between text-[11px]">
                  <span class="text-neutral-700">总计</span>
                  <span class="font-medium text-neutral-700 tabular-nums">{{ formatTokens(usageDetail.total) }}</span>
                </div>
              </div>
            </div>
          </NPopover>

          <!-- 模型 + 思考强度：Codex 风格 chip + 下钻浮层 -->
          <NPopover
            v-model:show="popoverOpen"
            trigger="click"
            placement="top-start"
            :show-arrow="false"
            :disabled="generating"
            raw
            class="model-popover"
          >
            <template #trigger>
              <button
                class="flex shrink-0 items-center gap-1 rounded-md px-1.5 py-1 text-xs text-neutral-600 transition-colors hover:bg-neutral-100"
                :class="generating ? 'cursor-not-allowed opacity-40' : 'cursor-pointer'"
                title="模型与思考强度"
              >
                <img
                  v-if="currentModelLogo"
                  :src="currentModelLogo"
                  alt=""
                  class="h-3.5 w-3.5 shrink-0 object-contain"
                />
                <Cpu v-else class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
                <span class="max-w-[140px] truncate">{{ currentModelLabel }}</span>
                <template v-if="effortConfig">
                  <span class="text-neutral-300">|</span>
                  <Gauge class="h-3.5 w-3.5 text-neutral-400" />
                  <span>{{ currentEffortLabel }}</span>
                </template>
                <ChevronDown class="h-3.5 w-3.5 text-neutral-400" />
              </button>
            </template>

            <div class="w-[280px] overflow-hidden rounded-lg border border-neutral-200 bg-white p-1 shadow-sm">
              <!-- root：下钻入口 -->
              <template v-if="panel === 'root'">
                <button
                  class="flex w-full cursor-pointer items-center gap-2 rounded-md px-2.5 py-2 text-left text-[13px] text-neutral-600 hover:bg-neutral-100"
                  @click="panel = 'model'"
                >
                  <span class="flex-1">模型</span>
                  <span class="max-w-[150px] truncate text-xs text-neutral-400">{{ currentModelLabel }}</span>
                  <ChevronRight class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
                </button>
                <button
                  v-if="effortConfig"
                  class="flex w-full cursor-pointer items-center gap-2 rounded-md px-2.5 py-2 text-left text-[13px] text-neutral-600 hover:bg-neutral-100"
                  @click="panel = 'effort'"
                >
                  <span class="flex-1">思考强度</span>
                  <span class="text-xs text-neutral-400">{{ currentEffortLabel }}</span>
                  <ChevronRight class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
                </button>
              </template>

              <!-- model 面板 -->
              <template v-else-if="panel === 'model'">
                <button
                  class="flex w-full cursor-pointer items-center gap-1 rounded-md px-2 py-1.5 text-left text-[13px] font-medium text-neutral-700 hover:bg-neutral-100"
                  @click="panel = 'root'"
                >
                  <ChevronLeft class="h-3.5 w-3.5 text-neutral-500" />
                  <span>选择模型</span>
                </button>
                <div class="relative px-1 pb-1">
                  <Search class="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-neutral-400" />
                  <input
                    v-model="modelSearch"
                    type="text"
                    placeholder="搜索模型"
                    class="w-full rounded-md border border-neutral-200 bg-neutral-50 py-1.5 pl-7 pr-2 text-xs text-neutral-700 outline-none placeholder:text-neutral-400 focus:border-neutral-300"
                  />
                </div>
                <div class="max-h-[280px] overflow-y-auto px-0.5">
                  <button
                    v-for="m in modelRows"
                    :key="m.value"
                    class="flex w-full cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-neutral-100"
                    @click="selectModel(m.value)"
                  >
                    <img
                      v-if="m.logo"
                      :src="m.logo"
                      alt=""
                      class="h-4 w-4 shrink-0 object-contain"
                    />
                    <Cpu v-else class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
                    <span class="min-w-0 flex-1 truncate" :class="m.value === currentModel ? 'text-blue-500' : 'text-neutral-600'">
                      {{ m.label }}
                    </span>
                    <Check v-if="m.value === currentModel" class="h-3.5 w-3.5 shrink-0 text-blue-500" />
                  </button>
                  <div v-if="!filteredModels.length" class="px-2 py-3 text-center text-xs text-neutral-400">
                    无匹配模型
                  </div>
                </div>
                <!-- 底部：重新获取模型（真实探针刷新） -->
                <div class="mt-0.5 border-t border-neutral-200 px-0.5 pt-1">
                  <button
                    class="flex w-full cursor-pointer items-center justify-center gap-1.5 rounded-md px-2 py-1.5 text-xs text-neutral-500 hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-40"
                    :disabled="refreshingModels"
                    title="从 Agent 重新获取模型列表"
                    @click="onRefreshModels"
                  >
                    <RefreshCw class="h-3.5 w-3.5" :class="refreshingModels ? 'animate-spin' : ''" />
                    <span>{{ refreshingModels ? '获取中…' : '重新获取模型' }}</span>
                  </button>
                </div>
              </template>

              <!-- effort 面板 -->
              <template v-else>
                <button
                  class="flex w-full cursor-pointer items-center gap-1 rounded-md px-2 py-1.5 text-left text-[13px] font-medium text-neutral-700 hover:bg-neutral-100"
                  @click="panel = 'root'"
                >
                  <ChevronLeft class="h-3.5 w-3.5 text-neutral-500" />
                  <span>思考强度</span>
                </button>
                <div class="px-0.5 pb-0.5">
                  <button
                    v-for="o in effortOptions"
                    :key="o.value"
                    class="flex w-full cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-neutral-100"
                    @click="selectEffort(o.value)"
                  >
                    <span class="min-w-0 flex-1 truncate" :class="o.value === currentEffort ? 'text-blue-500' : 'text-neutral-600'">
                      {{ o.label }}
                    </span>
                    <Check v-if="o.value === currentEffort" class="h-3.5 w-3.5 shrink-0 text-blue-500" />
                  </button>
                </div>
              </template>
            </div>
          </NPopover>

          <!-- 发送 / 停止 -->
          <button
            v-if="generating"
            class="flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-full bg-neutral-900 text-white hover:bg-neutral-700"
            title="停止生成"
            @click="stop(sessionId)"
          >
            <Square class="h-3.5 w-3.5" />
          </button>
          <button
            v-else
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-full transition-colors"
            :class="canSend ? 'cursor-pointer bg-neutral-900 text-white hover:bg-neutral-700' : 'cursor-not-allowed bg-neutral-100 text-neutral-300'"
            :disabled="!canSend"
            title="发送"
            @click="submit"
          >
            <ArrowUp class="h-4 w-4" />
          </button>
        </div>
      </div>

      <!-- 状态栏：项目 / 分支 … 上下文用量 -->
      <div class="mt-1.5 flex items-center gap-3 px-1 text-[11px] text-neutral-400">
        <span class="flex min-w-0 items-center gap-1">
          <Folder class="h-3 w-3 shrink-0" />
          <span class="truncate">{{ projectName }}</span>
        </span>
        <span class="flex min-w-0 items-center gap-1">
          <GitBranch class="h-3 w-3 shrink-0" />
          <span class="truncate">{{ branchName }}</span>
        </span>

        <div class="flex-1" />

        <span class="flex shrink-0 items-center gap-1 tabular-nums">
          <span
            class="inline-block h-2.5 w-2.5 rounded-full border border-neutral-300"
            :style="{
              background: `conic-gradient(#60a5fa ${contextPct}%, transparent ${contextPct}%)`,
            }"
          />
          {{ contextPct }}%
        </span>

        <!-- 连接健康：心形图标 + 弹层（进程状态 ready/dead/stopped） -->
        <NPopover
          v-model:show="healthOpen"
          trigger="click"
          placement="top-end"
          :show-arrow="false"
          raw
          class="connection-popover"
        >
          <template #trigger>
            <button
              class="flex shrink-0 cursor-pointer items-center gap-1 rounded-md px-1 py-0.5 transition-colors hover:bg-neutral-100"
              :title="`${agentName} · ${healthLabel}`"
            >
              <component :is="healthIcon" class="h-3.5 w-3.5" :class="healthClass" />
            </button>
          </template>

          <div class="w-[248px] overflow-hidden rounded-lg border border-neutral-200 bg-white p-3 shadow-sm">
            <!-- 头部：agent 名 + 状态 -->
            <div class="flex items-center justify-between gap-2">
              <span class="truncate text-xs font-medium text-neutral-700">{{ agentName }}</span>
              <span class="flex shrink-0 items-center gap-1 text-[11px]">
                <component :is="healthIcon" class="h-3 w-3" :class="healthClass" />
                <span :class="healthShimmer ? 'shimmer' : 'text-neutral-400'">{{ healthLabel }}</span>
              </span>
            </div>

            <!-- 错误原因（仅异常态） -->
            <p
              v-if="healthReason"
              class="mt-2 max-h-24 overflow-auto rounded-md bg-red-50 px-2 py-1 text-[11px] leading-snug break-words text-red-600"
            >
              {{ healthReason }}
            </p>

            <!-- 详情：工作目录 / 会话 ID -->
            <dl v-if="session?.cwd || session?.acpSessionId" class="mt-2 space-y-2">
              <div v-if="session?.cwd" class="space-y-0.5">
                <dt class="text-[11px] leading-none text-neutral-400">工作目录</dt>
                <dd class="font-mono text-[11px] leading-snug break-all text-neutral-600" :title="session.cwd">
                  {{ session.cwd }}
                </dd>
              </div>
              <div v-if="session?.acpSessionId" class="space-y-0.5">
                <dt class="text-[11px] leading-none text-neutral-400">会话 ID</dt>
                <dd
                  class="font-mono text-[11px] leading-snug break-all text-neutral-600"
                  :title="session.acpSessionId"
                >
                  {{ session.acpSessionId }}
                </dd>
              </div>
            </dl>

            <!-- 重新连接 -->
            <button
              class="mt-2.5 flex w-full cursor-pointer items-center justify-center gap-1.5 rounded-md border border-neutral-200 px-2 py-1.5 text-xs text-neutral-600 hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-40"
              :disabled="reconnecting"
              @click="onReconnect"
            >
              <RefreshCw class="h-3.5 w-3.5" :class="reconnecting && 'animate-spin'" />
              {{ reconnecting ? '重连中…' : '重新连接' }}
            </button>
          </div>
        </NPopover>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 无边框下拉：仅显示文字 + 自定义箭头，贴合 Codex 风格工具栏 */
.mode-select :deep(.n-base-selection) {
  --n-border: 0 !important;
  --n-border-hover: 0 !important;
  --n-border-focus: 0 !important;
  --n-box-shadow-focus: none !important;
  --n-color: transparent !important;
  --n-color-active: transparent !important;
  padding-left: 0;
}
.mode-select :deep(.n-base-selection-input__content) {
  color: #525252;
  font-size: 12px;
}
</style>
