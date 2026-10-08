<script setup lang="ts">
// Agent 管理分区：左「Agent 列表」+ 右「详情」两列（PRD R2）。
// 本期 OpenCode 为唯一可配置 Agent，其余为禁用占位行；
// 挂载即执行真实检测（design §2.1）。
import { onMounted } from 'vue'
import { useMessage } from 'naive-ui'
// lobehub 图标以 ?raw 内联 SVG 文本导入（currentColor，1em），v-html 渲染
import opencodeIcon from '@lobehub/icons-static-svg/icons/opencode.svg?raw'
import claudecodeIcon from '@lobehub/icons-static-svg/icons/claudecode-color.svg?raw'
import codexIcon from '@lobehub/icons-static-svg/icons/codex-color.svg?raw'
import geminiCliIcon from '@lobehub/icons-static-svg/icons/geminicli-color.svg?raw'
import clineIcon from '@lobehub/icons-static-svg/icons/cline.svg?raw'
import cursorIcon from '@lobehub/icons-static-svg/icons/cursor-text.svg?raw'
import type { AgentDef } from '../../../types/agent'
import { useAgentStore } from '../../../stores/agent'
import AgentListItem from './AgentListItem.vue'
import OpencodeDetail from './OpencodeDetail.vue'

const agentStore = useAgentStore()
const message = useMessage()

// Agent 全集（设计图清单）：本期仅 OpenCode 可检测/配置，其余禁用占位（PRD R2）
const agents: AgentDef[] = [
  {
    id: 'opencode',
    name: 'OpenCode',
    iconSvg: opencodeIcon,
    enabled: true,
    description: '支持 ACP 协议的开源编码 Agent',
  },
  { id: 'claudecode', name: 'Claude Code', iconSvg: claudecodeIcon, enabled: false, description: '' },
  { id: 'codex', name: 'Codex CLI', iconSvg: codexIcon, enabled: false, description: '' },
  { id: 'geminicli', name: 'Gemini CLI', iconSvg: geminiCliIcon, enabled: false, description: '' },
  { id: 'cline', name: 'Cline', iconSvg: clineIcon, enabled: false, description: '' },
  { id: 'cursor', name: 'Cursor', iconSvg: cursorIcon, enabled: false, description: '' },
]

const selectedAgentId = 'opencode'

function onSelect(agent: AgentDef) {
  if (!agent.enabled) {
    message.info(`「${agent.name}」即将推出`)
  }
}

async function refresh() {
  try {
    await agentStore.detect()
  } catch (e) {
    message.error(`检测失败：${e instanceof Error ? e.message : String(e)}`)
  }
}

onMounted(() => {
  // 首次进入执行检测；失败不弹错（预检面板会显示 FAIL 明细）
  agentStore.detect().catch(() => undefined)
})
</script>

<template>
  <div class="flex h-full min-h-0">
    <!-- 左列：Agent 列表 -->
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
    </aside>

    <!-- 右列：详情（本期仅 OpenCode） -->
    <OpencodeDetail class="min-w-0 flex-1" />
  </div>
</template>
