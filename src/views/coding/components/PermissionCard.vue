<script setup lang="ts">
// 权限审批卡（紧凑 dock 态）：照「删除会话?」确认框风格——
//   标题（工具中文名）+ 一行副标题（目标/命令）+ 右对齐按钮；
//   参数/diff/授予内容默认**收起**，点「详情」展开查看。
// 与 composer 融合（贴在其上沿），故卡片本身无下边框、圆角只在上方（见外层 dock 容器）。
import { computed, ref } from 'vue'
import { ShieldQuestion, ShieldAlert, ChevronRight, Check, X } from 'lucide-vue-next'
import type { AcpPermissionEvent, AcpPermissionOption } from '../../../types/acp'
import { useAcpPermission } from '../../../composables/useAcpPermission'
import {
  toolDisplayName,
  permissionTarget,
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

const title = computed(() => toolDisplayName(props.event))
const target = computed(() => permissionTarget(props.event))
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

/** rawInput JSON（详情面板的「原始参数」兜底） */
const rawJson = computed(() => {
  const raw = props.event.toolCall.rawInput
  if (raw === undefined || raw === null) return ''
  const text = typeof raw === 'string' ? raw : JSON.stringify(raw, null, 2)
  return text.length > 2000 ? `${text.slice(0, 2000)}…` : text
})
const hasDetail = computed(
  () => Boolean(command.value) || Boolean(diff.value) || hasAnyChanges.value || Boolean(rawJson.value),
)

const detailOpen = ref(false)

function isRejectOption(opt: AcpPermissionOption): boolean {
  return typeof opt.kind === 'string' && opt.kind.startsWith('reject')
}
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
const onOption = (opt: AcpPermissionOption) => run(() => respondOption(props.event, opt.optionId))
</script>

<template>
  <!-- 无自身圆角/下边框：由外层 dock 容器与 composer 融合（长出来的一体感） -->
  <div class="px-3 py-2">
    <!-- 头部：图标 + 工具名 + 副标题（一行）+ 待办计数 -->
    <div class="flex items-center gap-2">
      <component
        :is="defaultToNo ? ShieldAlert : ShieldQuestion"
        class="h-4 w-4 shrink-0"
        :class="defaultToNo ? 'text-amber-500' : 'text-blue-500'"
      />
      <span class="shrink-0 text-[13px] font-medium text-neutral-800">{{ title }}</span>
      <span v-if="target" class="min-w-0 flex-1 truncate text-[12px] text-neutral-400" :title="target">
        {{ target }}
      </span>
      <span v-else class="flex-1" />
      <span
        v-if="queuedCount && queuedCount > 0"
        class="shrink-0 rounded-md bg-neutral-100 px-1.5 py-0.5 text-[10px] tabular-nums text-neutral-500"
      >
        还有 {{ queuedCount }} 个
      </span>
      <button
        v-if="hasDetail"
        class="flex shrink-0 cursor-pointer items-center gap-0.5 rounded-md px-1.5 py-0.5 text-[11px] text-neutral-400 hover:bg-neutral-100 hover:text-neutral-600"
        @click="detailOpen = !detailOpen"
      >
        <ChevronRight class="h-3 w-3 transition-transform" :class="detailOpen && 'rotate-90'" />
        详情
      </button>
    </div>

    <!-- 理由（_meta.permission.description，一行，若有） -->
    <p
      v-if="description && !detailOpen"
      class="mt-0.5 truncate pl-6 text-[11px] text-neutral-400"
      :title="description"
    >
      {{ description }}
    </p>

    <!-- 详情（默认收起）：命令 / diff / 授予内容 / 原始参数 -->
    <div v-if="detailOpen" class="mt-1.5 space-y-1.5 pl-6">
      <div v-if="command" class="rounded-md border border-neutral-200 bg-neutral-50 p-2">
        <pre class="max-h-32 overflow-auto whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-neutral-700">{{ command }}</pre>
      </div>
      <DiffPreview v-if="diff" :diff="diff" />
      <div v-if="hasAnyChanges" class="rounded-md border border-neutral-200 bg-neutral-50 p-2">
        <div class="mb-1 text-[11px] text-neutral-400">选中后会授予</div>
        <div v-for="opt in event.options" :key="opt.optionId">
          <template v-if="optionChanges[opt.optionId]?.length">
            <div class="text-[11px] font-medium text-neutral-700">{{ optionLabel(opt) }}</div>
            <div
              v-for="(change, i) in optionChanges[opt.optionId]"
              :key="i"
              class="flex items-start gap-2 text-[11px]"
            >
              <span v-if="change.scope" class="shrink-0 rounded border border-neutral-300 px-1 text-[10px] text-neutral-500">
                {{ CHANGE_SCOPE_LABEL[change.scope] }}
              </span>
              <span class="min-w-0 break-words text-neutral-600">{{ change.description }}</span>
            </div>
          </template>
        </div>
      </div>
      <pre
        v-else-if="rawJson && !command && !diff"
        class="max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-md border border-neutral-200 bg-neutral-50 p-2 font-mono text-[11px] leading-5 text-neutral-600"
      >{{ rawJson }}</pre>
    </div>

    <!-- 按钮：右对齐（照确认框风格） -->
    <div class="mt-2 flex justify-end gap-2">
      <template v-if="hasOptions">
        <button
          v-for="(opt, i) in event.options"
          :key="opt.optionId"
          class="flex h-7 cursor-pointer items-center gap-1 rounded-lg px-2.5 text-xs disabled:cursor-not-allowed disabled:opacity-50"
          :class="
            isEmphasized(opt, i)
              ? 'bg-neutral-900 text-white hover:bg-neutral-700'
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
      </template>
      <template v-else>
        <button
          class="flex h-7 cursor-pointer items-center gap-1 rounded-lg bg-neutral-900 px-2.5 text-xs text-white hover:bg-neutral-700 disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="busy"
          @click="onAllow"
        >
          允许一次
        </button>
        <button
          class="flex h-7 cursor-pointer items-center gap-1 rounded-lg border border-neutral-300 bg-white px-2.5 text-xs text-neutral-600 hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="busy"
          @click="onAllowAlways"
        >
          本会话总是允许
        </button>
        <button
          class="flex h-7 cursor-pointer items-center gap-1 rounded-lg border border-red-200 bg-white px-2.5 text-xs text-red-600 hover:bg-red-50 disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="busy"
          @click="onDeny"
        >
          拒绝
        </button>
      </template>
    </div>
  </div>
</template>
