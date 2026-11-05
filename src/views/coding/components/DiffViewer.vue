<script setup lang="ts">
// monacoSetup 必须是首行 import：Monaco 求值前先装好 worker 工厂。
import '../monacoSetup'
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
      :options="{
        renderSideBySide: false,
        readOnly: false,
        minimap: { enabled: false },
        scrollbar: {
          verticalScrollbarSize: 8,
          horizontalScrollbarSize: 8,
          useShadows: false,
          verticalSliderSize: 8,
          horizontalSliderSize: 8,
          alwaysConsumeMouseWheel: false,
        },
        overviewRulerLanes: 0,
        hideCursorInOverviewRuler: true,
        overviewRulerBorder: false,
      }"
    />
  </div>
</template>
