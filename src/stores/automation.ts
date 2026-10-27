// 自动化仓库：列表的内存副本 + 服务层调用。
//
// 边界（state-management spec）：所有写操作 await automationService 后用**返回值**
// 更新本地行，不做乐观更新 —— 状态与时间戳的权威在 Rust（调度器会自己推进）。
// 组件永远不 invoke。运行历史按自动化 id 缓存（进历史弹窗时才拉）。

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { Automation, AutomationDraft, AutomationRun } from '../types/automation'
import { automationService } from '../services/automationService'

export const useAutomationStore = defineStore('automation', () => {
  const items = ref<Automation[]>([])
  const loaded = ref(false)
  const loading = ref(false)

  /** 运行历史缓存：automationId → 最近一次拉取的记录 */
  const runsById = ref<Record<string, AutomationRun[]>>({})

  const byId = computed(() => {
    const map = new Map<string, Automation>()
    for (const a of items.value) map.set(a.id, a)
    return map
  })

  /** 未读失败总数（侧栏 / 标题徽标） */
  const unseenFailures = computed(() =>
    items.value.reduce((sum, a) => sum + a.unseenFailures, 0),
  )

  function get(id: string | null | undefined): Automation | null {
    return id ? (byId.value.get(id) ?? null) : null
  }

  /** 用返回的行替换本地同一 id（找不到则插到最前） */
  function replaceRow(row: Automation): void {
    const idx = items.value.findIndex((a) => a.id === row.id)
    if (idx >= 0) items.value[idx] = row
    else items.value.unshift(row)
  }

  /**
   * 整表重读。事件到达时也走这条（design §5.10：不做增量合并）。
   *
   * `loading` 只用于首次加载的骨架展示，避免每次事件刷新都闪一下。
   */
  async function load(force = false): Promise<void> {
    if (loaded.value && !force) return
    loading.value = !loaded.value
    try {
      items.value = await automationService.list()
      loaded.value = true
    } finally {
      loading.value = false
    }
  }

  /**
   * 整表重读。
   *
   * **同时刷新已缓存的运行历史** —— 否则「运行中」会永远挂着：
   * `automation://changed` 只在 run 结算时到达一次，如果这里只刷 items，
   * 缓存里那条 `running` 记录就再也没人更新，卡片会一直显示「运行中」。
   */
  async function refresh(): Promise<void> {
    items.value = await automationService.list()
    loaded.value = true
    // 只刷新「已经拉过历史」的自动化，不为没打开过的去多发请求
    const cached = Object.keys(runsById.value)
    await Promise.all(
      cached.map(async (id) => {
        try {
          runsById.value[id] = await automationService.runs(id, 30)
        } catch {
          // 刷新失败保留旧数据：下次事件或重新打开弹窗会再拉
        }
      }),
    )
  }

  async function create(draft: AutomationDraft): Promise<Automation> {
    const row = await automationService.create(draft)
    replaceRow(row)
    return row
  }

  async function update(id: string, draft: AutomationDraft): Promise<Automation> {
    const row = await automationService.update(id, draft)
    replaceRow(row)
    return row
  }

  async function setEnabled(id: string, enabled: boolean): Promise<Automation> {
    const row = await automationService.setEnabled(id, enabled)
    replaceRow(row)
    return row
  }

  async function remove(id: string): Promise<void> {
    await automationService.remove(id)
    items.value = items.value.filter((a) => a.id !== id)
    delete runsById.value[id]
  }

  /** 立即运行：返回新运行 id；失败会把错误原样抛出（由页面 toast） */
  async function runNow(id: string): Promise<string> {
    const runId = await automationService.runNow(id)
    // 后端会 emit 事件让列表刷新，这里顺手把历史缓存作废
    delete runsById.value[id]
    return runId
  }

  async function cancelRun(automationId: string, runId: string): Promise<void> {
    await automationService.cancelRun(runId)
    delete runsById.value[automationId]
  }

  /** 拉取某自动化的运行历史（进弹窗时调用） */
  async function loadRuns(id: string, limit = 20): Promise<AutomationRun[]> {
    const rows = await automationService.runs(id, limit)
    runsById.value[id] = rows
    return rows
  }

  function runsOf(id: string): AutomationRun[] {
    return runsById.value[id] ?? []
  }

  /** 清零未读失败：本地立即归零（徽标不用等一次往返），再落盘 */
  async function markSeen(): Promise<void> {
    if (unseenFailures.value === 0) return
    for (const a of items.value) a.unseenFailures = 0
    await automationService.markSeen()
  }

  return {
    items,
    loaded,
    loading,
    runsById,
    unseenFailures,
    get,
    load,
    refresh,
    create,
    update,
    setEnabled,
    remove,
    runNow,
    cancelRun,
    loadRuns,
    runsOf,
    markSeen,
  }
})
