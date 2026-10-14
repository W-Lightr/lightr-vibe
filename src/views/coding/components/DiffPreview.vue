<script setup lang="ts">
// 轻量 unified-diff 预览：按行渲染 +/-/上下文，绿/红/灰着色。无第三方依赖。
// 仅用于权限卡内的 edit 审批预览（不编辑、不定位真实文件）。
import { computed } from 'vue'

const props = defineProps<{ diff: string }>()

interface DiffLine {
  kind: 'add' | 'del' | 'meta' | 'ctx'
  text: string
}

/** 解析 unified diff：`@@` 行 → meta；`+`/`-` 行 → add/del；其余 → ctx。
 *  `+++`/`---` 文件头归入 meta（不当成增删行）。 */
const lines = computed<DiffLine[]>(() => {
  return props.diff.split(/\r?\n/).map((raw) => {
    if (raw.startsWith('+++') || raw.startsWith('---')) return { kind: 'meta', text: raw }
    if (raw.startsWith('@@')) return { kind: 'meta', text: raw }
    if (raw.startsWith('+')) return { kind: 'add', text: raw }
    if (raw.startsWith('-')) return { kind: 'del', text: raw }
    return { kind: 'ctx', text: raw }
  })
})

function lineClass(kind: DiffLine['kind']): string {
  switch (kind) {
    case 'add':
      return 'bg-green-50 text-green-800'
    case 'del':
      return 'bg-red-50 text-red-800'
    case 'meta':
      return 'bg-neutral-100 text-neutral-400'
    case 'ctx':
      return 'text-neutral-600'
  }
}
</script>

<template>
  <pre
    class="max-h-48 overflow-auto rounded-md border border-neutral-200 bg-white py-1 font-mono text-[11px] leading-5"
  ><code
      v-for="(l, i) in lines"
      :key="i"
      class="block whitespace-pre px-2"
      :class="lineClass(l.kind)"
    >{{ l.text || ' ' }}</code></pre>
</template>
