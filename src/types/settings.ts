// 设置页域类型：分区 id 与菜单条目。
// why 放 types/：<script setup> 不允许 export（Vue SFC 限制），
// 且 type-safety.md 要求域类型集中在 src/types/。

/** 设置分区 id：本期仅 'agent' 真实可用，其余为禁用占位（design D4 预留） */
export type SettingsSectionId =
  | 'agent'
  | 'appearance'
  | 'general'
  | 'collaboration'
  | 'mcp'
  | 'skills'
  | 'skill-packs'
  | 'model-providers'
  | 'quick-messages'
  | 'browser'
  | 'computer'
  | 'shortcuts'
  | 'version-control'
  | 'message-channels'
  | 'web-service'
  | 'logs'
  | 'system'
