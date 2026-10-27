// 自动化服务层：automation_* 命令的唯一 invoke 入口（tauri-bridge spec §2 边界）。
//
// isTauri() 双通道：Tauri → Rust invoke（~/.lightr-vibe/lightr-vibe/automations.json），
// 浏览器 dev → automationMock 内存实现。组件 / store 永远不直接 invoke。
//
// 事件：Rust 侧所有写操作都会 emit `automation://changed`（payload 只带 id + runId），
// 前端收到就整表重读（design §5.10）。

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import type {
  Automation,
  AutomationDraft,
  AutomationRun,
  NextRunPreview,
} from '../types/automation'
import { isTauri } from '../utils/isTauri'
import {
  mockCancelRun,
  mockComputeNextRun,
  mockCreate,
  mockGet,
  mockList,
  mockMarkSeen,
  mockRunNow,
  mockRuns,
  mockSetEnabled,
  mockSoftDelete,
  mockUpdate,
} from './automationMock'

/** 变更事件名（与 Rust `AUTOMATION_CHANGED_EVENT` 一致） */
export const AUTOMATION_CHANGED_EVENT = 'automation://changed'

/** 事件载荷：只带 id，前端整表重读 */
export interface AutomationChangedPayload {
  id: string
  runId: string | null
}

export const automationService = {
  /** 列出全部未软删的自动化（按 updatedAt 倒序） */
  async list(): Promise<Automation[]> {
    if (!isTauri()) return mockList()
    return invoke<Automation[]>('automation_list')
  },

  async get(id: string): Promise<Automation> {
    if (!isTauri()) return mockGet(id)
    return invoke<Automation>('automation_get', { id })
  },

  async create(draft: AutomationDraft): Promise<Automation> {
    if (!isTauri()) return mockCreate(draft)
    return invoke<Automation>('automation_create', { draft })
  },

  async update(id: string, draft: AutomationDraft): Promise<Automation> {
    if (!isTauri()) return mockUpdate(id, draft)
    return invoke<Automation>('automation_update', { id, draft })
  },

  async setEnabled(id: string, enabled: boolean): Promise<Automation> {
    if (!isTauri()) return mockSetEnabled(id, enabled)
    return invoke<Automation>('automation_set_enabled', { id, enabled })
  },

  async remove(id: string): Promise<void> {
    if (!isTauri()) {
      mockSoftDelete(id)
      return
    }
    await invoke('automation_delete', { id })
  },

  /** 清零全部未读失败（进入自动化页时调用） */
  async markSeen(): Promise<void> {
    if (!isTauri()) {
      mockMarkSeen()
      return
    }
    await invoke('automation_mark_seen')
  },

  /** 编辑器的「下次运行」预览：与真实调度器共用同一个求值器 */
  async computeNextRun(cron: string, timezone: string): Promise<NextRunPreview> {
    if (!isTauri()) return mockComputeNextRun(cron, timezone)
    return invoke<NextRunPreview>('automation_compute_next_run', { cron, timezone })
  },

  /** 某个自动化的运行历史（倒序取最近 limit 条） */
  async runs(id: string, limit?: number): Promise<AutomationRun[]> {
    if (!isTauri()) return mockRuns(id, limit)
    return invoke<AutomationRun[]>('automation_runs', { id, limit: limit ?? null })
  },

  /** 手动「立即运行」，返回新运行 id */
  async runNow(id: string): Promise<string> {
    if (!isTauri()) return mockRunNow(id)
    return invoke<string>('automation_run_now', { id })
  },

  /** 取消一个在飞运行 */
  async cancelRun(runId: string): Promise<void> {
    if (!isTauri()) {
      mockCancelRun(runId)
      return
    }
    await invoke('automation_cancel_run', { runId })
  },

  /**
   * 订阅变更事件。浏览器 dev 下返回一个空 unlisten（mock 自己维护状态）。
   *
   * 调用方负责在 unmount 时调用返回的函数。
   */
  async onChanged(handler: (payload: AutomationChangedPayload) => void): Promise<UnlistenFn> {
    if (!isTauri()) return () => {}
    return listen<AutomationChangedPayload>(AUTOMATION_CHANGED_EVENT, (event) => {
      handler(event.payload)
    })
  },
}
