<script setup lang="ts">
// Agent 管理分区：左「Agent 列表」+ 右「详情」两列（PRD R2/R7）。
// 列表由引擎注册表 store 驱动（acp_list_agents，前端不写死）；本期仅 OpenCode 可配置，
// 其余引擎为禁用占位行。新增「默认引擎」选择，写 settings.json（engine_default_set）。
import { computed, onMounted, ref } from 'vue'
import { useMessage } from 'naive-ui'
// lobehub 图标以 ?raw 内联 SVG 文本导入（currentColor，1em），v-html 渲染
import opencodeIcon from '@lobehub/icons-static-svg/icons/opencode.svg?raw'
import claudecodeIcon from '@lobehub/icons-static-svg/icons/claudecode-color.svg?raw'
import codexIcon from '@lobehub/icons-static-svg/icons/codex-color.svg?raw'
import geminiCliIcon from '@lobehub/icons-static-svg/icons/geminicli-color.svg?raw'
import clineIcon from '@lobehub/icons-static-svg/icons/cline.svg?raw'
import cursorIcon from '@lobehub/icons-static-svg/icons/cursor-text.svg?raw'
import type { AgentDef } from '../../../types/agent'
import type { EngineWire } from '../../../types/engine'
import { useAgentStore } from '../../../stores/agent'
import { useEngineStore } from '../../../stores/engine'
import AgentListItem from './AgentListItem.vue'
import OpencodeDetail from './OpencodeDetail.vue'

const agentStore = useAgentStore()
const engineStore = useEngineStore()
const message = useMessage()

/** wire → 图标（未知引擎用空串占位），前端仅负责视觉映射，不决定列表成员 */
const ENGINE_ICONS: Record<string, string> = {
  open_code: opencodeIcon,
  claude_code: claudecodeIcon,
  codex: codexIcon,
  gemini: geminiCliIcon,
  cline: clineIcon,
  cursor: cursorIcon,
}

/** 注册表已包含的引擎 wire 集合（用于判定占位行是否已可配置） */
const registeredWires = computed(() => new Set<string>(engineStore.agents.map((a) => a.agentType)))

/** 左列：以注册表列表为准，再追加本期未注册的占位 Agent（设计图清单，禁用） */
const agents = computed<AgentDef[]>(() => {
  const fromRegistry: AgentDef[] = engineStore.agents.map((a) => ({
    id: a.agentType,
    name: a.name,
    iconSvg: ENGINE_ICONS[a.agentType] ?? '',
    enabled: a.enabled,
    description: a.description,
  }))
  const placeholders: AgentDef[] = [
    { id: 'claude_code', name: 'Claude Code', iconSvg: claudecodeIcon, enabled: false, description: '' },
    { id: 'codex', name: 'Codex CLI', iconSvg: codexIcon, enabled: false, description: '' },
    { id: 'gemini', name: 'Gemini CLI', iconSvg: geminiCliIcon, enabled: false, description: '' },
    { id: 'cline', name: 'Cline', iconSvg: clineIcon, enabled: false, description: '' },
    { id: 'cursor', name: 'Cursor', iconSvg: cursorIcon, enabled: false, description: '' },
  ].filter((p) => !registeredWires.value.has(p.id))
  return [...fromRegistry, ...placeholders]
})

/** 当前选中引擎（响应式；默认聚焦有注册表的首个，否则首个占位） */
const selectedAgentId = ref<string>(engineStore.agents[0]?.agentType ?? 'open_code')

/** 详情区是否展示 OpenCode 专用面板（本期唯一实现的引擎） */
const showOpencodeDetail = computed(() => selectedAgentId.value === 'open_code')

function onSelect(agent: AgentDef) {
  if (!agent.enabled) {
    message.info(`「${agent.name}」即将推出`)
    return
  }
  selectedAgentId.value = agent.id
}

async function refresh() {
  try {
    await agentStore.detect(selectedAgentId.value as EngineWire)
  } catch (e) {
    message.error(`检测失败：${e instanceof Error ? e.message : String(e)}`)
  }
}

/** 「默认引擎」选择：写 settings.json（engine_default_set），失败回滚提示 */
async function onDefaultEngineChange(wire: string) {
  try {
    await engineStore.setDefaultEngine(wire as EngineWire)
    message.success(`默认引擎已设为「${engineStore.agentName(wire)}」`)
  } catch (e) {
    message.error(`保存默认引擎失败：${e instanceof Error ? e.message : String(e)}`)
  }
}

onMounted(() => {
  // 拉取注册表（失败保留缓存/兜底）；随后检测当前引擎（预检面板显示 FAIL 明细）
  void engineStore.load()
  agentStore.detect().catch(() => undefined)
})
</script>

<template>
  <div class="flex h-full min-h-0">
    <!-- 左列：Agent 列表 + 默认引擎 -->
    <aside class="flex w-64 shrink-0 flex-col border-r border-neutral-200 bg-white">
      <div class="px-3 pb-2 pt-3 text-xs font-medium text-neutral-500">Agent 列表</div>
      <div class="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-2 pb-2">
        <AgentListItem
          v-for="agent in agents"
          :key="agent.id"
          :agent="agent"
          :status="agent.enabled ? agentStore.agentStatus : 'unknown'"
          :selected="agent.enabled && agent.id === selectedAgentId"
          @select="onSelect(agent)"
          @refresh="agent.enabled && refresh()"
        />
      </div>

      <!-- 默认引擎：新建会话弹窗的预选项 -->
      <div class="border-t border-neutral-200 p-3">
        <div class="mb-1.5 text-xs font-medium text-neutral-500">默认引擎</div>
        <select
          class="w-full cursor-pointer rounded-md border border-neutral-300 bg-white px-2 py-1.5 text-[13px] text-neutral-700 outline-none focus:border-blue-400"
          :value="engineStore.defaultEngineId"
          @change="onDefaultEngineChange(($event.target as HTMLSelectElement).value)"
        >
          <option v-for="agent in engineStore.agents" :key="agent.agentType" :value="agent.agentType">
            {{ agent.name }}
          </option>
        </select>
        <p class="mt-1.5 text-[11px] text-neutral-400">新建会话时默认预选该引擎</p>
      </div>
    </aside>

    <!-- 右列：详情（本期仅 OpenCode） -->
    <OpencodeDetail v-if="showOpencodeDetail" class="min-w-0 flex-1" />
    <div v-else class="flex min-w-0 flex-1 items-center justify-center text-sm text-neutral-400">
      该引擎详情即将推出
    </div>
  </div>
</template>
