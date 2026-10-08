<script setup lang="ts">
// 环境变量卡片（PRD R6）：KEY=VALUE 行编辑 + 多行兜底 textarea 切换。
// 保存写入应用级 ~/.lightr-vibe/agent-env.json —— 官方 schema 无全局 env 对象
// （research §3 / design D3），供 Lightr-vibe 后续启动 Agent 进程时注入，
// 不改动 opencode 自身配置（说明文案在界面明示）。
import { computed, ref, watch } from 'vue'
import { useMessage } from 'naive-ui'
import { LoaderCircle, Plus, Save, Trash2 } from 'lucide-vue-next'
import { useAgentStore } from '../../../stores/agent'
import type { EnvVar } from '../../../types/agent'

const agentStore = useAgentStore()
const message = useMessage()

/** 行编辑草稿（与 store 隔离，保存成功后才提交） */
const rows = ref<EnvVar[]>([])
/** 多行兜底模式：直接编辑 KEY=VALUE 文本 */
const rawMode = ref(false)
const rawText = ref('')

// store 数据加载/保存成功后整体替换 envVars（新数组引用），同步到本地草稿
watch(
  () => agentStore.envVars,
  () => {
    rows.value = agentStore.envVars.map((v) => ({ ...v }))
    rawText.value = agentStore.envVars.map((v) => `${v.key}=${v.value}`).join('\n')
  },
  { immediate: true },
)

/** 待保存行（过滤空 key） */
const effectiveRows = computed(() => rows.value.filter((r) => r.key.trim()))

function addRow() {
  rows.value.push({ key: '', value: '' })
}

function removeRow(index: number) {
  rows.value.splice(index, 1)
}

/** 多行文本 → 行数据：每行 KEY=VALUE（首个 = 分隔），# 开头为注释 */
function parseRawText(text: string): EnvVar[] {
  return text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith('#'))
    .map((line) => {
      const idx = line.indexOf('=')
      return idx === -1 ? { key: line, value: '' } : { key: line.slice(0, idx), value: line.slice(idx + 1) }
    })
}

function toggleRawMode() {
  if (!rawMode.value) {
    // 行 → 文本
    rawText.value = rows.value
      .filter((r) => r.key.trim())
      .map((r) => `${r.key.trim()}=${r.value}`)
      .join('\n')
  } else {
    // 文本 → 行
    rows.value = parseRawText(rawText.value)
  }
  rawMode.value = !rawMode.value
}

async function save() {
  // 兜底模式下先解析文本为行
  if (rawMode.value) {
    rows.value = parseRawText(rawText.value)
  }
  const err = await agentStore.saveEnv(effectiveRows.value)
  if (err) {
    message.error(`保存失败：${err.kind === 'conflict' ? '写入冲突，请重试' : err.message}`)
    return
  }
  message.success('环境变量已保存')
}
</script>

<template>
  <section class="rounded-md border border-neutral-200 bg-white">
    <div class="flex items-center justify-between border-b border-neutral-200 px-3 py-2">
      <div>
        <span class="text-xs font-medium text-neutral-600">环境变量</span>
        <p class="mt-0.5 text-[11px] text-neutral-400">
          保存至应用级 ~/.lightr-vibe/agent-env.json，由 Lightr-vibe 在启动 Agent
          进程时注入；opencode 自身配置不会被修改。
        </p>
      </div>
      <button
        class="shrink-0 cursor-pointer rounded-md border border-neutral-300 px-2 py-1 text-xs text-neutral-600 transition-colors hover:bg-neutral-100"
        :title="rawMode ? '切换为行编辑' : '切换为多行文本编辑'"
        @click="toggleRawMode"
      >
        {{ rawMode ? '行编辑' : '多行编辑' }}
      </button>
    </div>

    <div class="flex flex-col gap-2 p-3">
      <!-- 行编辑模式 -->
      <template v-if="!rawMode">
        <div v-if="rows.length === 0" class="text-xs text-neutral-400">
          暂无环境变量，点击下方「添加变量」新增。
        </div>
        <div v-for="(row, index) in rows" :key="index" class="flex items-center gap-1.5">
          <input
            v-model="row.key"
            class="h-7 w-40 shrink-0 rounded-md border border-neutral-300 px-2 font-mono text-xs outline-none focus:border-blue-400"
            placeholder="KEY"
            data-no-drag
          />
          <span class="shrink-0 text-xs text-neutral-400">=</span>
          <input
            v-model="row.value"
            class="h-7 min-w-0 flex-1 rounded-md border border-neutral-300 px-2 font-mono text-xs outline-none focus:border-blue-400"
            placeholder="VALUE"
            data-no-drag
          />
          <button
            class="flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-neutral-400 hover:bg-red-50 hover:text-red-500"
            title="删除该行"
            @click="removeRow(index)"
          >
            <Trash2 class="h-3.5 w-3.5" />
          </button>
        </div>
        <button
          class="flex w-fit cursor-pointer items-center gap-1 rounded-md border border-neutral-300 px-2 py-1 text-xs text-neutral-600 transition-colors hover:bg-neutral-100"
          @click="addRow"
        >
          <Plus class="h-3.5 w-3.5" />
          添加变量
        </button>
      </template>

      <!-- 多行兜底模式：每行 KEY=VALUE，# 开头为注释 -->
      <textarea
        v-else
        v-model="rawText"
        class="h-40 w-full resize-y rounded-md border border-neutral-300 p-2 font-mono text-xs outline-none focus:border-blue-400"
        placeholder="每行一个 KEY=VALUE，# 开头为注释"
        spellcheck="false"
        data-no-drag
      />

      <!-- 保存按钮：设计图样式黑底（implement C16） -->
      <div class="flex items-center gap-2">
        <button
          class="flex cursor-pointer items-center gap-1.5 rounded-md bg-neutral-900 px-3 py-1.5 text-xs text-white transition-colors hover:bg-neutral-700 disabled:cursor-not-allowed disabled:opacity-40"
          :disabled="agentStore.writing"
          @click="save"
        >
          <LoaderCircle v-if="agentStore.writing" class="h-3.5 w-3.5 animate-spin" />
          <Save v-else class="h-3.5 w-3.5" />
          保存环境变量
        </button>
        <span class="text-[11px] text-neutral-400">
          生效变量 {{ effectiveRows.length }} 个
        </span>
      </div>
    </div>
  </section>
</template>
