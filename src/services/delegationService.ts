// Delegation Tauri 双通道服务；组件与 store 不直接调用 invoke。
import { invoke } from '@tauri-apps/api/core'
import type { DelegationRecord, DelegationSettings } from '../types/delegation'
import { isTauri } from '../utils/isTauri'
import { delegationMock } from './delegationMock'

export const delegationService = {
  async getSettings(): Promise<DelegationSettings> {
    return isTauri() ? invoke<DelegationSettings>('delegation_get_settings') : delegationMock.getSettings()
  },
  async setSettings(settings: DelegationSettings): Promise<DelegationSettings> {
    return isTauri() ? invoke<DelegationSettings>('delegation_set_settings', { settings }) : delegationMock.setSettings(settings)
  },
  async list(): Promise<DelegationRecord[]> {
    return isTauri() ? invoke<DelegationRecord[]>('delegation_list') : delegationMock.list()
  },
}
