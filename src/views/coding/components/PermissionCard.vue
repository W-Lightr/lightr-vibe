<script setup lang="ts">
// 权限审批卡：富信息版。
// - 标题/理由：优先 `_meta`（claudeCode.title / permission.title / permission.description）。
// - 身体：命令 / diff（edit 审批）/ 选项授予内容（`_meta.permission.changes[]`）。
// - 按钮：由 options[].kind 动态渲染（文案优先 agent 给的 option.name）；
//   `defaultToNo` 时强调「拒绝」。
// - 无 options / 无 _meta 时回退旧三态（允许一次 / 本会话总是允许 / 拒绝）。
// ui-style：neutral 卡 + blue 主按钮 + red 拒绝 + rounded-lg + text-[13px]。
import { computed, ref } from 'vue'
import { ShieldQuestion, ShieldAlert, Check, CheckCheck, X, Terminal, KeyRound } from 'lucide-vue-next'
import type { AcpPermissionEvent, AcpPermissionOption } from '../../../types/acp'
import { useAcpPermission } from '../../../composables/useAcpPermission'
import {
  parsePermissionTitle,
  parsePermissionDescription,
  isDefaultToNo,
  parsePermissionCommand,
  parseOptionChanges,
  extractDiff,
  optionLabel,
  CHANGE_SCOPE_LABEL,
  type PermissionOptionChange,
} from '../../../utils/permissionMeta'
import DiffPreview from './DiffPreview.vue'

const props = defineProps<{ event: AcpPermissionEvent; queuedCount?: number }>()

const { allow, allowAlways, deny, respondOption } = useAcpPermission()

const title = computed(() => parsePermissionTitle(props.event))
const description = computed(() => parsePermissionDescription(props.event))
const defaultToNo = computed(() => isDefaultToNo(props.event))
const command = computed(() => parsePermissionCommand(props.event))
const diff = computed(() => extractDiff(props.event.toolCall.rawInput))

/** 有 agent 提供的 options 才走动态按钮；否则回退旧三态 */
const hasOptions = computed(() => props.event.options.length > 0)

/** 每个选项的授予内容（changes[]），key = optionId */
const optionChanges = computed<Record<string, PermissionOptionChange[]>>(() => {
  const out: Record<string, PermissionOptionChange[]> = {}
  for (const opt of props.event.options) out[opt.optionId] = parseOptionChanges(opt)
  return out
})

const hasAnyChanges = computed(() =>
  Object.values(optionChanges.value).some((list) => list.length > 0),
)

/** rawInput JSON 摘要（无结构化视图时的兜底） */
const inputSummary = computed(() => {
  const raw = props.event.toolCall.rawInput
  if (raw === undefined || raw === null) return ''
  const text = typeof raw === 'string' ? raw : JSON.stringify(raw)
  return text.length > 160 ? `${text.slice(0, 160)}…` : text
})

function isRejectOption(opt: AcpPermissionOption): boolean {
  return typeof opt.kind === 'string' && opt.kind.startsWith('reject')
}

/** 动态按钮的强调：defaultToNo 时强调 reject，否则强调非 reject 的首个 */
function isEmphasized(opt: AcpPermissionOption, index: number): boolean {
  const reject = isRejectOption(opt)
  if (defaultToNo.value) return reject
  if (reject) return false
  return props.event.options.findIndex((o) => !isRejectOption(o)) === index
}

const busy = ref(false)

async function run(fn: () => Promise<void>) {
  if (busy.value) return
  busy.value = true
  try {
    await fn()
  } finally {
    busy.value = false
  }
}

const onAllow = () => run(() => allow(props.event))
const onAllowAlways = () => run(() => allowAlways(props.event))
const onDeny = () => run(() => deny(props.event))
const onOption = (opt: AcpPermissionOption) =>
  run(() => respondOption(props.event, opt.optionId))
</script>

<template>
  <div class="mb-2 rounded-xl border border-neutral-200/70 bg-white p-3 shadow-sm">
    <!-- 头部：图标 + 标题 + 理由 + 待办计数 -->
    <div class="flex items-start justify-between gap-2">
      <div class="min-w-0">
        <div class="flex items-center gap-2 text-[13px] text-neutral-800">
          <component
            :is="defaultToNo ? ShieldAlert : ShieldQuestion"
            class="h-4 w-4 shrink-0"
            :class="defaultToNo ? 'text-amber-500' : 'text-blue-500'"
          />
          <span class="truncate font-medium">{{ title }}</span>
        </div>
        <p v-if="description" class="mt-0.5 text-[11px] leading-snug text-neutral-500">
          {{ description }}
        </p>
      </div>
      <span
        v-if="queuedCount && queuedCount > 0"
        class="shrink-0 rounded-md bg-neutral-200 px-1.5 py-0.5 text-[10px] tabular-nums text-neutral-500"
        title="同会话还有其它待审批请求"
      >
        还有 {{ queuedCount }} 个待审批
      </span>
    </div>

    <div class="mt-2 space-y-2">
      <!-- 命令 -->
      <div v-if="command" class="rounded-md border border-neutral-200 bg-white p-2">
        <div class="flex items-center gap-1 text-[11px] text-neutral-400">
          <Terminal class="h-3.5 w-3.5" />
          <span>命令</span>
        </div>
        <pre class="mt-1 max-h-32 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-neutral-700">{{ command }}</pre>
      </div>

      <!-- diff（edit 审批） -->
      <DiffPreview v-if="diff" :diff="diff" />

      <!-- 无结构化视图时的 rawInput 摘要 -->
      <pre
        v-else-if="inputSummary && !command"
        class="max-h-24 overflow-auto whitespace-pre-wrap break-all rounded-md border border-neutral-200 bg-white p-2 font-mono text-[11px] leading-5 text-neutral-600"
      >{{ inputSummary }}</pre>

      <!-- 选项授予内容（_meta.permission.changes） -->
      <div v-if="hasAnyChanges" class="rounded-md border border-neutral-200 bg-white p-2">
        <div class="flex items-center gap-1 text-[11px] text-neutral-400">
          <KeyRound class="h-3.5 w-3.5" />
          <span>选中后会授予</span>
        </div>
        <div class="mt-1 space-y-2">
          <div v-for="opt in event.options" :key="opt.optionId" class="space-y-1">
            <template v-if="optionChanges[opt.optionId]?.length">
              <div class="text-[11px] font-medium text-neutral-700">
                {{ optionLabel(opt) }}
              </div>
              <div
                v-for="(change, i) in optionChanges[opt.optionId]"
                :key="i"
                class="flex items-start gap-2 text-[11px]"
              >
                <span
                  v-if="change.scope"
                  class="shrink-0 rounded border border-neutral-300 px-1 text-[10px] text-neutral-500"
                >
                  {{ CHANGE_SCOPE_LABEL[change.scope] }}
                </span>
                <span class="min-w-0 break-words text-neutral-600">{{ change.description }}</span>
              </div>
            </template>
          </div>
        </div>
      </div>
    </div>

    <!-- 按钮：有 options 走动态；否则回退旧三态 -->
    <div v-if="hasOptions" class="mt-2.5 flex flex-wrap gap-2">
      <button
        v-for="(opt, i) in event.options"
        :key="opt.optionId"
        class="flex h-7 cursor-pointer items-center gap-1 rounded-md px-2.5 text-xs disabled:cursor-not-allowed disabled:opacity-50"
        :class="
          isEmphasized(opt, i)
            ? 'bg-blue-500 text-white hover:bg-blue-600'
            : isRejectOption(opt)
              ? 'border border-red-200 bg-white text-red-600 hover:bg-red-50'
              : 'border border-neutral-300 bg-white text-neutral-600 hover:bg-neutral-100'
        "
        :disabled="busy"
        @click="onOption(opt)"
      >
        <Check v-if="!isRejectOption(opt)" class="h-3.5 w-3.5" />
        <X v-else class="h-3.5 w-3.5" />
        {{ optionLabel(opt) }}
      </button>
    </div>
    <div v-else class="mt-2.5 flex items-center gap-2">
      <button
        class="flex h-7 cursor-pointer items-center gap-1 rounded-md bg-blue-500 px-2.5 text-xs text-white hover:bg-blue-600 disabled:cursor-not-allowed disabled:opacity-50"
        :disabled="busy"
        @click="onAllow"
      >
        <Check class="h-3.5 w-3.5" />
        允许一次
      </button>
      <button
        class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-neutral-300 bg-white px-2.5 text-xs text-neutral-600 hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-50"
        :disabled="busy"
        @click="onAllowAlways"
      >
        <CheckCheck class="h-3.5 w-3.5" />
        本会话总是允许
      </button>
      <button
        class="flex h-7 cursor-pointer items-center gap-1 rounded-md border border-red-200 bg-white px-2.5 text-xs text-red-600 hover:bg-red-50 disabled:cursor-not-allowed disabled:opacity-50"
        :disabled="busy"
        @click="onDeny"
      >
        <X class="h-3.5 w-3.5" />
        拒绝
      </button>
    </div>
  </div>
</template>
