<script setup lang="ts">
import { ref } from 'vue'
import MonacoEditor from 'monaco-editor-vue'

// 示例 diff 数据（后续由 sidecar 的文件修改事件驱动）
const original = ref(`function greet(name: string) {
  return "Hello, " + name
}
`)
const modified = ref(`function greet(name: string): string {
  return \`Hello, \${name}!\`
}

export { greet }
`)
</script>

<template>
  <div class="h-full">
    <MonacoEditor
      v-model="modified"
      diff-editor
      :original="original"
      language="typescript"
      theme="vs"
      height="100%"
      width="100%"
      :options="{ renderSideBySide: false, readOnly: false, minimap: { enabled: false } }"
    />
  </div>
</template>
