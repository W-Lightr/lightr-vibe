<script setup lang="ts">
// Agent 管理分区：左「Agent 列表」+ 右「详情」两列（PRD R2/R7）。
// 列表由引擎注册表 store 驱动（acp_list_agents，前端不写死）；本期仅 OpenCode 可配置，
// 其余引擎为禁用占位行。新增「默认引擎」选择，写 settings.json（engine_default_set）。
import { computed, onMounted, ref, watch } from 'vue'
import { useMessage } from 'naive-ui'
import { engineIcon } from '../../../utils/engineIcons'
import type { AgentDef } from '../../../types/agent'
import type { EngineWire } from '../../../types/engine'
import { BUILTIN_ENGINE_WIRES } from '../../../types/engine'
import { useAgentStore } from '../../../stores/agent'
import { useEngineStore } from '../../../stores/engine'
import AgentListItem from './AgentListItem.vue'
import OpencodeDetail from './OpencodeDetail.vue'
import EngineDetail from './EngineDetail.vue'

const agentStore = useAgentStore()
const engineStore = useEngineStore()
const message = useMessage()

/** 注册表已包含的引擎 wire 集合（用于判定占位行是否已可配置） */
const registeredWires = computed(() => new Set<string>(engineStore.agents.map((a) => a.agentType)))

/** 左列：以注册表列表为准，再追加本期未注册的占位 Agent（设计图清单，禁用） */
const agents = computed<AgentDef[]>(() => {
  const fromRegistry: AgentDef[] = engineStore.agents.map((a) => ({
    id: a.agentType,
    name: a.name,
    iconSvg: engineIcon(a.agentType),
    enabled: a.enabled,
    description: a.description,
  }))
  const placeholders: AgentDef[] = [
    { id: 'claude_code', name: 'Claude Code', iconSvg: engineIcon('claude_code'), enabled: false, description: '' },
    { id: 'codex', name: 'Codex CLI', iconSvg: engineIcon('codex'), enabled: false, description: '' },
    { id: 'gemini', name: 'Gemini CLI', iconSvg: engineIcon('gemini'), enabled: false, description: '' },
    { id: 'cline', name: 'Cline', iconSvg: engineIcon('cline'), enabled: false, description: '' },
    { id: 'cursor', name: 'Cursor', iconSvg: engineIcon('cursor'), enabled: false, description: '' },
  ].filter((p) => !registeredWires.value.has(p.id))
  return [...fromRegistry, ...placeholders]
})

/** 当前选中引擎（响应式；默认聚焦有注册表的首个，否则首个占位） */
const selectedAgentId = ref<string>(engineStore.agents[0]?.agentType ?? 'open_code')

/** 详情区是否展示 OpenCode 专用面板（唯一带配置编辑的引擎） */
const showOpencodeDetail = computed(() => selectedAgentId.value === 'open_code')

/** 选中引擎的注册表条目（详情头图标/描述用；占位行可能无条目） */
const selectedAgent = computed(() => agents.value.find((a) => a.id === selectedAgentId.value) ?? null)

/**
 * 选中项写入 store.activeEngineId（detectResult/prechecks 的取数键）。
 * 消除「selectedAgentId 与 activeEngineId 不联动」的既有缺口（tauri-bridge spec 已知 gap）。
 */
watch(selectedAgentId, (id) => {
  if ((BUILTIN_ENGINE_WIRES as string[]).includes(id)) {
    agentStore.activeEngineId = id as EngineWire
  }
})

function onSelect(agent: AgentDef) {
  if (!agent.enabled) {
    message.info(`「${agent.name}」即将推出`)
    return
  }
  selectedAgentId.value = agent.id
  // 切换引擎后按新选中项重新检测（预检/徽章对应该引擎）
  if ((BUILTIN_ENGINE_WIRES as string[]).includes(agent.id)) {
    void refresh(agent.id)
  }
}

/** 重新检测指定引擎（列表行「刷新」按钮只测自己那一行，不牵连当前选中项）。
 *  重入由 store 的 detecting 锁兜底：正在检测时直接反馈，避免静默丢失点击。 */
async function refresh(engineId: string) {
  if (agentStore.detecting) {
    message.info('检测正在进行，请稍候')
    return
  }
  try {
    await agentStore.detect(engineId as EngineWire)
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

onMounted(async () => {
  // 先拉注册表，再按注册表逐个检测（徽章各引擎独立，不共用一次检测结果）。
  // detectAll 只按当前聚焦引擎加载一次配置/环境变量，避免后测的引擎把先测的缓存清掉。
  try {
    await engineStore.load()
    // 初次聚焦即写 activeEngineId（watch 不覆盖初值），保证详情与列表选中一致
    if ((BUILTIN_ENGINE_WIRES as string[]).includes(selectedAgentId.value)) {
      agentStore.activeEngineId = selectedAgentId.value as EngineWire
    }
    await agentStore.detectAll(engineStore.agents.map((a) => a.agentType))
  } catch (e) {
    // runDetect 失败会向上抛（loadSupporting 已自行降级）：不能让 onMounted 成为
    // 未处理的 rejection，至少给出可见反馈。
    message.error(`引擎检测失败：${e instanceof Error ? e.message : String(e)}`)
  }
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
          :status="agent.enabled ? agentStore.agentStatusFor(agent.id) : 'unknown'"
          :selected="agent.enabled && agent.id === selectedAgentId"
          @select="onSelect(agent)"
          @refresh="agent.enabled && refresh(agent.id)"
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

    <!-- 右列：详情（OpenCode 专用面板 / 其余引擎通用占位） -->
    <OpencodeDetail v-if="showOpencodeDetail" class="min-w-0 flex-1" />
    <EngineDetail
      v-else-if="selectedAgent?.enabled"
      :engine-id="selectedAgentId as EngineWire"
      :icon-svg="selectedAgent.iconSvg"
      class="min-w-0 flex-1 overflow-y-auto"
    />
    <div v-else class="flex min-w-0 flex-1 items-center justify-center text-sm text-neutral-400">
      该引擎详情即将推出
    </div>
  </div>
</template>
