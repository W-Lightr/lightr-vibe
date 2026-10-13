// 引擎注册表 store（照 codeg use-acp-agents 裁剪）：单一共享 store，
// 挂载时经 acp_list_agents 拉取、排序（sortOrder + name）、缓存；失败保留上一次列表。
// 默认引擎读写写入 settings.json（engineService 双通道）。
// 组件不直接调 service；列表权威来自 Rust 注册表，前端不写死。

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { AcpAgentInfo, EngineWire } from '../types/engine'
import { BUILTIN_ENGINE_WIRES, DEFAULT_ENGINE_WIRE } from '../types/engine'
import { engineService } from '../services/engineService'

/** 静态兜底列表：Tauri 缺席 / 拉取失败且无缓存时使用（照 codeg 静态回退）。 */
const FALLBACK_AGENTS: AcpAgentInfo[] = [
  {
    agentType: 'open_code',
    name: 'OpenCode',
    description: '支持 ACP（Agent Client Protocol）的开源编码 Agent',
    enabled: true,
    sortOrder: 0,
    available: true,
    installedVersion: null,
  },
]

/** sortOrder 升序 + name 字典序（照 codeg 排序规则）。 */
function sortAgents(list: AcpAgentInfo[]): AcpAgentInfo[] {
  return [...list].sort((a, b) => a.sortOrder - b.sortOrder || a.name.localeCompare(b.name))
}

export const useEngineStore = defineStore('engine', () => {
  /** 引擎列表（首次加载前为静态兜底，界面不空窗） */
  const agents = ref<AcpAgentInfo[]>(sortAgents(FALLBACK_AGENTS))
  /** 是否已成功从 Rust 拉取过至少一次（fresh = 数据新鲜） */
  const fresh = ref(false)
  const loading = ref(false)
  /** 默认引擎 wire（设置页「默认引擎」） */
  const defaultEngineId = ref<EngineWire>(DEFAULT_ENGINE_WIRE)
  /** 用户本次会话在弹窗中的选择（不写回默认；fallback 不算用户确认） */
  const lastPickedEngineId = ref<EngineWire | null>(null)

  /** 可用（检测通过）的引擎 */
  const availableAgents = computed(() => agents.value.filter((a) => a.available))

  /** 默认引擎条目（缺失时 null） */
  const defaultAgent = computed(
    () => agents.value.find((a) => a.agentType === defaultEngineId.value) ?? null,
  )

  /**
   * 新建会话弹窗的预选引擎（含 fallback 语义，照 codeg AgentSelector）：
   * 默认引擎可用 → 用它；否则回退首个 available；都没有 → 首个引擎。**不写回默认**
   * （fallback 不是用户确认）。
   */
  const preselectedEngineId = computed<EngineWire>(() => {
    const def = defaultAgent.value
    if (def?.available) return def.agentType
    const firstAvail = availableAgents.value[0]
    if (firstAvail) return firstAvail.agentType
    return agents.value[0]?.agentType ?? DEFAULT_ENGINE_WIRE
  })

  /** 拉取引擎列表（失败保留上一次列表，不因瞬时失败清空；照 codeg）。 */
  async function load(): Promise<void> {
    if (loading.value) return
    loading.value = true
    try {
      const list = await engineService.list()
      if (list.length) {
        agents.value = sortAgents(list)
        fresh.value = true
      }
      const def = await engineService.defaultGet()
      defaultEngineId.value = def
    } catch {
      // 失败保留既有（兜底/上次）列表；fresh 维持原值
    } finally {
      loading.value = false
    }
  }

  /** 手动刷新（等同 load，保留语义入口） */
  async function refresh(): Promise<void> {
    await load()
  }

  /** 设置默认引擎（写 settings.json；成功后更新内存态） */
  async function setDefaultEngine(engineId: EngineWire): Promise<void> {
    await engineService.defaultSet(engineId)
    defaultEngineId.value = engineId
  }

  /** 记录弹窗中的本次选择（不写回默认） */
  function pickEngine(engineId: EngineWire): void {
    if ((BUILTIN_ENGINE_WIRES as string[]).includes(engineId)) {
      lastPickedEngineId.value = engineId
    }
  }

  /** 引擎显示名（无匹配/未加载时回落 OpenCode 字面，避免 UI 空窗，R6） */
  function agentName(engineId?: EngineWire | string | null): string {
    const found = agents.value.find((a) => a.agentType === engineId)
    return found?.name ?? 'OpenCode'
  }

  return {
    agents,
    fresh,
    loading,
    defaultEngineId,
    lastPickedEngineId,
    availableAgents,
    defaultAgent,
    preselectedEngineId,
    load,
    refresh,
    setDefaultEngine,
    pickEngine,
    agentName,
  }
})
