<script setup lang="ts">
// 权限设置卡片（PRD R5）：官方 schema 的 15 个 permission 键 × ask/allow/deny 三态。
// 未设置的键显示「默认（继承）」空态，选择后才写入 opencode.jsonc；
// 清空选择 = 删除该键恢复继承（research §3 键名清单）。
import { computed } from 'vue'
import { NSelect, useMessage } from 'naive-ui'
import { useAgentStore } from '../../../stores/agent'
import { PERMISSION_KEYS, type PermissionAction } from '../../../types/agent'

const agentStore = useAgentStore()
const message = useMessage()

const actionOptions: { label: string; value: PermissionAction }[] = [
  { label: '询问', value: 'ask' },
  { label: '允许', value: 'allow' },
  { label: '拒绝', value: 'deny' },
]

/** 当前权限映射：单一 action 字符串写法（'*'）时展开到每个键展示 */
const resolved = computed<Record<string, PermissionAction | null>>(() => {
  const permissions = agentStore.configView?.permissions ?? {}
  const wildcard = permissions['*'] ?? null
  const result: Record<string, PermissionAction | null> = {}
  for (const { key } of PERMISSION_KEYS) {
    result[key] = permissions[key] ?? wildcard
  }
  return result
})

async function onChange(key: string, action: PermissionAction | null) {
  const err = await agentStore.savePermission(key, action)
  if (err) {
    message.error(
      err.kind === 'conflict'
        ? '配置已被外部修改，请点击「刷新目录」后重试'
        : `保存失败：${err.message}`,
    )
    return
  }
  message.success(action ? `「${labelOf(key)}」已设为${actionLabel(action)}` : `「${labelOf(key)}」已恢复默认（继承）`)
}

function labelOf(key: string): string {
  return PERMISSION_KEYS.find((k) => k.key === key)?.label ?? key
}

function actionLabel(action: PermissionAction): string {
  return actionOptions.find((o) => o.value === action)?.label ?? action
}
</script>

<template>
  <section class="rounded-md border border-neutral-200 bg-white">
    <div class="border-b border-neutral-200 px-3 py-2">
      <span class="text-xs font-medium text-neutral-600">权限设置</span>
      <p class="mt-0.5 text-[11px] text-neutral-400">
        写入 opencode.jsonc 的 permission 对象；「默认（继承）」表示不显式设置该键。
      </p>
    </div>

    <div class="grid grid-cols-2 gap-x-3 gap-y-1.5 p-3 xl:grid-cols-3">
      <label v-for="entry in PERMISSION_KEYS" :key="entry.key" class="flex items-center gap-2">
        <span class="w-24 shrink-0 truncate text-xs text-neutral-600" :title="`${entry.label}（${entry.key}）`">
          {{ entry.label }}
        </span>
        <NSelect
          :value="resolved[entry.key] ?? null"
          :options="actionOptions"
          size="small"
          clearable
          :disabled="agentStore.writing"
          placeholder="默认（继承）"
          class="min-w-0 flex-1"
          @update:value="(v: PermissionAction | null) => onChange(entry.key, v ?? null)"
        />
      </label>
    </div>
  </section>
</template>
