import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { DelegationRecord, DelegationSettings } from '../types/delegation'
import { delegationService } from '../services/delegationService'

/** 子代理 broker 的前端投影；不拥有 ACP 消息或子会话正文。 */
export const useDelegationStore = defineStore('delegation', () => {
  const settings = ref<DelegationSettings>({ enabled: false, depthLimit: 1 })
  const records = ref<DelegationRecord[]>([])
  const byTaskId = computed(() => new Map(records.value.map((record) => [record.taskId, record])))

  async function load(): Promise<void> {
    const [nextSettings, nextRecords] = await Promise.all([delegationService.getSettings(), delegationService.list()])
    settings.value = nextSettings
    records.value = nextRecords
  }

  async function saveSettings(next: DelegationSettings): Promise<void> {
    settings.value = await delegationService.setSettings(next)
  }

  /** broker started/updated 事件使用相同的 upsert，避免卡片状态分叉。 */
  function upsert(record: DelegationRecord): void {
    const index = records.value.findIndex((item) => item.taskId === record.taskId)
    if (index === -1) records.value.unshift(record)
    else records.value.splice(index, 1, record)
  }

  function get(taskId: string | null | undefined): DelegationRecord | null {
    return taskId ? (byTaskId.value.get(taskId) ?? null) : null
  }

  return { settings, records, get, load, saveSettings, upsert }
})
