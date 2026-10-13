<script setup lang="ts">
// 新建会话弹窗：选择引擎后创建会话（PRD R5）。
// 列表来自 engine store（经 acp_list_agents 下发，前端不写死）；预选 = 默认引擎，
// 默认引擎不可用时 fallback 首个 available（照 codeg AgentSelector：fallback 不写回默认）。
import { computed, onMounted, ref } from 'vue'
import { ChevronDown, X } from 'lucide-vue-next'
// lobehub 图标以 ?raw 内联 SVG 文本导入（currentColor，1em），v-html 渲染（与 AgentManageView 一致）
import opencodeIcon from '@lobehub/icons-static-svg/icons/opencode.svg?raw'
import claudecodeIcon from '@lobehub/icons-static-svg/icons/claudecode-color.svg?raw'
import codexIcon from '@lobehub/icons-static-svg/icons/codex-color.svg?raw'
import geminiCliIcon from '@lobehub/icons-static-svg/icons/geminicli-color.svg?raw'
import clineIcon from '@lobehub/icons-static-svg/icons/cline.svg?raw'
import cursorIcon from '@lobehub/icons-static-svg/icons/cursor-text.svg?raw'
import { useEngineStore } from '../../../stores/engine'
import type { EngineWire } from '../../../types/engine'

/** 引擎 wire → 图标 SVG（未知引擎空串占位；与 AgentManageView 同一映射） */
const ENGINE_ICONS: Record<string, string> = {
  open_code: opencodeIcon,
  claude_code: claudecodeIcon,
  codex: codexIcon,
  gemini: geminiCliIcon,
  cline: clineIcon,
  cursor: cursorIcon,
}
function engineIcon(wire: string): string {
  return ENGINE_ICONS[wire] ?? ''
}

const props = defineProps<{
  /** 弹窗是否可见（父级 v-model 控制） */
  open: boolean
}>()

const emit = defineEmits<{
  close: []
  /** 用户确认：携带所选引擎 wire（父级据此 createSession） */
  confirm: [engineId: EngineWire]
}>()

const engineStore = useEngineStore()

/** 本次选择（默认预选；打开时同步为 preselectedEngineId） */
const selected = ref<EngineWire>(engineStore.preselectedEngineId)

// 弹窗打开时同步预选并拉取最新列表（失败保留缓存）
onMounted(() => {
  void engineStore.load()
})

function syncOnOpen() {
  selected.value = engineStore.preselectedEngineId
}

const list = computed(() => engineStore.agents)

/** 当前选中引擎条目（收起态展示图标 + 名称） */
const selectedAgent = computed(
  () => list.value.find((x) => x.agentType === selected.value) ?? null,
)

/** 下拉展开态 */
const dropOpen = ref(false)

function choose(agentType: EngineWire) {
  selected.value = agentType
  dropOpen.value = false
}

/** 点击弹窗其它区域时收起下拉 */
function onRootMousedown() {
  dropOpen.value = false
}

const canConfirm = computed(() => {
  const a = list.value.find((x) => x.agentType === selected.value)
  return !!a && a.enabled
})

function confirm() {
  if (!canConfirm.value) return
  engineStore.pickEngine(selected.value)
  emit('confirm', selected.value)
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
      class="w-[340px] rounded-md border border-neutral-200 bg-white p-4"
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
          class="flex w-full cursor-pointer items-center gap-2 rounded-md border border-neutral-300 bg-white px-2.5 py-2 text-left transition-colors hover:bg-neutral-50"
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
          class="absolute z-10 mt-1 max-h-60 w-full overflow-y-auto rounded-md border border-neutral-200 bg-white py-1 shadow-sm"
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

      <div class="mt-4 flex justify-end gap-2">
        <button
          class="cursor-pointer rounded-md border border-neutral-200 px-3 py-1.5 text-[13px] text-neutral-600 transition-colors hover:bg-neutral-100"
          @click="close"
        >
          取消
        </button>
        <button
          class="cursor-pointer rounded-md bg-blue-500 px-3 py-1.5 text-[13px] text-white transition-colors hover:bg-blue-600 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="!canConfirm"
          @click="confirm"
        >
          创建
        </button>
      </div>
    </div>
  </div>
</template>
