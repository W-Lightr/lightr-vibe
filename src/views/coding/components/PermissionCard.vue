<script setup lang="ts">
// 权限审批卡（confirm 模式）：工具名 / input 摘要 / 允许一次 / 本会话总是允许 / 拒绝。
// ui-style 合规：neutral 卡 + blue 主按钮 + red 拒绝 + rounded-md + text-[13px]。
import { computed, ref } from 'vue'
import { ShieldQuestion, Check, CheckCheck, X } from 'lucide-vue-next'
import type { AcpPermissionEvent } from '../../../types/acp'
import { useAcpPermission } from '../../../composables/useAcpPermission'

const props = defineProps<{ event: AcpPermissionEvent }>()

const { allow, allowAlways, deny } = useAcpPermission()

const toolName = computed(() => props.event.toolCall.title ?? props.event.toolCall.toolCallId ?? '未知工具')

/** input 摘要：rawInput JSON 截断展示（最多 2 行视觉高度） */
const inputSummary = computed(() => {
  const raw = props.event.toolCall.rawInput
  if (raw === undefined || raw === null) return ''
  const text = typeof raw === 'string' ? raw : JSON.stringify(raw)
  return text.length > 160 ? `${text.slice(0, 160)}…` : text
})

const busy = ref(false)

async function onAllow() {
  if (busy.value) return
  busy.value = true
  try {
    await allow(props.event)
  } finally {
    busy.value = false
  }
}

async function onAllowAlways() {
  if (busy.value) return
  busy.value = true
  try {
    await allowAlways(props.event)
  } finally {
    busy.value = false
  }
}

async function onDeny() {
  if (busy.value) return
  busy.value = true
  try {
    await deny(props.event)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="mb-4 rounded-md border border-neutral-200 bg-neutral-50/60 px-3 py-2.5">
    <div class="flex items-center gap-2 text-[13px] text-neutral-800">
      <ShieldQuestion class="h-4 w-4 shrink-0 text-blue-500" />
      <span class="font-medium">Agent 请求执行工具</span>
      <code class="rounded bg-neutral-200/70 px-1.5 py-0.5 font-mono text-[11px] text-neutral-700">{{ toolName }}</code>
    </div>

    <pre
      v-if="inputSummary"
      class="mt-2 max-h-24 overflow-auto whitespace-pre-wrap break-all rounded-md border border-neutral-200 bg-white p-2 font-mono text-[11px] leading-5 text-neutral-600"
    >{{ inputSummary }}</pre>

    <div class="mt-2.5 flex items-center gap-2">
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
