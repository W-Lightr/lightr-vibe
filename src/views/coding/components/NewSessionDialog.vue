<script setup lang="ts">
// 新建会话弹窗：选择引擎后创建会话（PRD R5）。
// 列表来自 engine store（经 acp_list_agents 下发，前端不写死）；预选 = 默认引擎，
// 默认引擎不可用时 fallback 首个 available（照 codeg AgentSelector：fallback 不写回默认）。
import { computed, onMounted, ref } from 'vue'
import { X } from 'lucide-vue-next'
import { useEngineStore } from '../../../stores/engine'
import type { EngineWire } from '../../../types/engine'

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
      @mousedown.stop
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
      <div class="flex flex-col gap-1">
        <button
          v-for="agent in list"
          :key="agent.agentType"
          class="flex cursor-pointer flex-col items-start gap-0.5 rounded-md border px-2.5 py-2 text-left transition-colors"
          :class="
            selected === agent.agentType
              ? 'border-blue-400 bg-blue-50/50'
              : 'border-neutral-200 hover:bg-neutral-100'
          "
          :disabled="!agent.enabled"
          @click="selected = agent.agentType"
        >
          <div class="flex w-full items-center justify-between gap-2">
            <span class="text-[13px] text-neutral-800">{{ agent.name }}</span>
            <span v-if="!agent.available" class="text-[10px] text-amber-500">未安装</span>
          </div>
          <span v-if="agent.description" class="text-[11px] text-neutral-500">
            {{ agent.description }}
          </span>
        </button>
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
