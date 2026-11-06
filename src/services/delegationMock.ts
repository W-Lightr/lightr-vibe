import type { DelegationRecord, DelegationSettings } from '../types/delegation'

let settings: DelegationSettings = { enabled: false, depthLimit: 1 }
const records: DelegationRecord[] = []

export const delegationMock = {
  async getSettings(): Promise<DelegationSettings> { return { ...settings } },
  async setSettings(next: DelegationSettings): Promise<DelegationSettings> {
    settings = { enabled: next.enabled, depthLimit: Math.max(1, Math.min(4, next.depthLimit)) }
    return { ...settings }
  },
  async list(): Promise<DelegationRecord[]> { return [...records] },
}
