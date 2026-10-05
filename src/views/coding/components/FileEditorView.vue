<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import MonacoEditor from 'monaco-editor-vue'
import type { OpenTab } from '../../../types/session'
import { useProjectStore } from '../../../stores/project'
import { languageByPath } from '../../../utils/file'

const props = defineProps<{ tab: OpenTab }>()

const projectStore = useProjectStore()

const path = computed(() => props.tab.filePath ?? '')
const language = computed(() => languageByPath(path.value))
const content = ref('')

// 打开 / 切换文件标签时从项目内存文件系统载入内容
function load() {
  if (props.tab.projectId && props.tab.filePath) {
    content.value = projectStore.getFileContent(props.tab.projectId, props.tab.filePath)
  }
}
load()
watch(
  () => props.tab.id,
  () => load(),
)

// 编辑内容写回项目 store（Mock 保存）
function onChange(value: string) {
  if (props.tab.projectId && props.tab.filePath) {
    projectStore.setFileContent(props.tab.projectId, props.tab.filePath, value)
  }
}
</script>

<template>
  <div class="flex h-full flex-col bg-white">
    <MonacoEditor
      :model-value="content"
      :language="language"
      theme="vs"
      height="100%"
      width="100%"
      :options="{ minimap: { enabled: false }, fontSize: 13, tabSize: 2, wordWrap: 'on' }"
      @change="onChange"
    />
  </div>
</template>
