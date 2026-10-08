<script setup lang="ts">
// 预检面板：4 项折叠行（chevron + label + PASS/FAIL 徽章）+「预检项：N」+ 诊断按钮。
// 数据来自 store.prechecks（由 DetectResult 映射，design §2.1）。
import { ref } from 'vue'
import { ChevronDown, ChevronRight, LoaderCircle, Stethoscope } from 'lucide-vue-next'
import type { PrecheckStatus } from '../../../types/agent'
import { useAgentStore } from '../../../stores/agent'

const agentStore = useAgentStore()

/** 折叠状态：默认展开全部（信息密度优先，ui-style.md） */
const expanded = ref<Record<string, boolean>>({})

function toggle(id: string) {
  expanded.value[id] = !(expanded.value[id] ?? true)
}

function isOpen(id: string): boolean {
  return expanded.value[id] ?? true
}

function badgeClass(status: PrecheckStatus): string {
  switch (status) {
    case 'pass':
      return 'bg-green-50 text-green-600'
    case 'fail':
      return 'bg-red-50 text-red-600'
    case 'running':
      return 'bg-neutral-100 text-neutral-500'
    case 'unknown':
      return 'bg-neutral-100 text-neutral-400'
  }
}

function badgeText(status: PrecheckStatus): string {
  switch (status) {
    case 'pass':
      return 'PASS'
    case 'fail':
      return 'FAIL'
    case 'running':
      return '检测中'
    case 'unknown':
      return '未知'
  }
}

async function diagnose() {
  await agentStore.detect()
}

</script>

<template>
  <section class="rounded-md border border-neutral-200 bg-white">
    <!-- 头部：预检项计数 + 诊断按钮 -->
    <div class="flex items-center justify-between border-b border-neutral-200 px-3 py-2">
      <span class="text-xs font-medium text-neutral-600">
        预检项：{{ agentStore.prechecks.length }}
      </span>
      <button
        class="flex cursor-pointer items-center gap-1 rounded-md border border-neutral-300 px-2 py-1 text-xs text-neutral-600 transition-colors hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-40"
        :disabled="agentStore.detecting"
        title="逐项重新执行检测"
        @click="diagnose"
      >
        <LoaderCircle v-if="agentStore.detecting" class="h-3.5 w-3.5 animate-spin text-blue-500" />
        <Stethoscope v-else class="h-3.5 w-3.5" />
        诊断
      </button>
    </div>

    <!-- 未检测时的空态 -->
    <div v-if="agentStore.prechecks.length === 0" class="px-3 py-4 text-xs text-neutral-400">
      {{ agentStore.detecting ? '正在检测…' : '尚未执行检测，点击「诊断」开始' }}
    </div>

    <!-- 预检项折叠列表 -->
    <ul v-else class="divide-y divide-neutral-100">
      <li v-for="item in agentStore.prechecks" :key="item.id">
        <button
          class="flex w-full cursor-pointer items-center gap-1.5 px-3 py-1.5 text-left transition-colors hover:bg-neutral-50"
          @click="toggle(item.id)"
        >
          <ChevronDown v-if="isOpen(item.id)" class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
          <ChevronRight v-else class="h-3.5 w-3.5 shrink-0 text-neutral-400" />
          <span class="min-w-0 flex-1 truncate text-xs text-neutral-600">{{ item.label }}</span>
          <span
            class="shrink-0 rounded-md px-1.5 py-0.5 text-[10px] font-medium"
            :class="badgeClass(item.status)"
          >
            {{ badgeText(item.status) }}
          </span>
        </button>
        <!-- 明细：路径/版本/失败原因（font-mono 展示路径类信息） -->
        <div v-if="isOpen(item.id)" class="px-3 pb-2 pl-9 text-[11px] break-all text-neutral-500">
          <span class="font-mono">{{ item.detail }}</span>
        </div>
      </li>
    </ul>
  </section>
</template>
