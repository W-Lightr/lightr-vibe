<script setup lang="ts">
// 设置左菜单：本期仅「Agent 管理」真实可用；其余设计图条目以禁用态呈现，
// 点击给中文占位反馈（PRD R1，与 TopBar notImplemented 惯例一致）。
import { useMessage } from 'naive-ui'
import {
  Bot,
  Brush,
  Cog,
  GitBranch,
  Globe,
  Keyboard,
  Monitor,
  Package,
  Palette,
  Plug,
  Radio,
  ScrollText,
  Server,
  Settings2,
  ShieldCheck,
  Sparkles,
  Users,
} from 'lucide-vue-next'
import type { Component } from 'vue'
import type { SettingsSectionId } from '../../../types/settings'

interface MenuEntry {
  id: SettingsSectionId
  label: string
  icon: Component
  /** false = 禁用占位行，点击提示「即将推出」 */
  enabled: boolean
}

const props = defineProps<{ activeSection: SettingsSectionId }>()
const emit = defineEmits<{ 'update:activeSection': [value: SettingsSectionId] }>()

const message = useMessage()

// 设计图菜单全集（PRD R1 枚举），顺序与设计图一致
const entries: MenuEntry[] = [
  { id: 'agent', label: 'Agent 管理', icon: Bot, enabled: true },
  { id: 'appearance', label: '外观', icon: Palette, enabled: false },
  { id: 'general', label: '常规', icon: Cog, enabled: false },
  { id: 'collaboration', label: '协作', icon: Users, enabled: false },
  { id: 'mcp', label: 'MCP', icon: Plug, enabled: false },
  { id: 'skills', label: 'Skills', icon: Sparkles, enabled: false },
  { id: 'skill-packs', label: '技能包', icon: Package, enabled: false },
  { id: 'model-providers', label: '模型供应商', icon: Brush, enabled: false },
  { id: 'quick-messages', label: '快捷消息', icon: Radio, enabled: false },
  { id: 'browser', label: '浏览器操作', icon: Globe, enabled: false },
  { id: 'computer', label: '电脑操作', icon: Monitor, enabled: false },
  { id: 'shortcuts', label: '快捷键', icon: Keyboard, enabled: false },
  { id: 'version-control', label: '版本控制', icon: GitBranch, enabled: false },
  { id: 'message-channels', label: '消息渠道', icon: Server, enabled: false },
  { id: 'web-service', label: 'Web 服务', icon: Settings2, enabled: false },
  { id: 'logs', label: '运行日志', icon: ScrollText, enabled: false },
  { id: 'system', label: '系统', icon: ShieldCheck, enabled: false },
]

function onSelect(entry: MenuEntry) {
  if (!entry.enabled) {
    message.info(`「${entry.label}」即将推出`)
    return
  }
  emit('update:activeSection', entry.id)
}

</script>

<template>
  <!-- 左菜单：白底细边框分隔，选中态 bg-neutral-200/70（ui-style.md） -->
  <nav class="flex w-44 shrink-0 flex-col gap-0.5 overflow-y-auto border-r border-neutral-200 bg-white p-2">
    <button
      v-for="entry in entries"
      :key="entry.id"
      class="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] transition-colors"
      :class="
        entry.enabled
          ? props.activeSection === entry.id
            ? 'bg-neutral-200/70 font-medium text-neutral-800'
            : 'text-neutral-600 hover:bg-neutral-100'
          : 'cursor-not-allowed text-neutral-400 opacity-40'
      "
      :title="entry.enabled ? entry.label : `${entry.label}（即将推出）`"
      @click="onSelect(entry)"
    >
      <component :is="entry.icon" class="h-4 w-4 shrink-0 text-neutral-500" />
      <span class="min-w-0 flex-1 truncate">{{ entry.label }}</span>
    </button>
  </nav>
</template>
