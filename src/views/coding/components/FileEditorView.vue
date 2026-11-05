<script setup lang="ts">
// 文件编辑器：fs_read_file 真实读取 → Monaco；dirty 跟踪（基线 = 加载时
// mtime + text，design D5）；保存（按钮 + Ctrl+S）走 fs_write_file 带基线
// mtime 冲突校验；conflict → naive-ui 对话框「重新加载 / 覆盖」二选一，
// 绝不静默覆盖（R5）。too-large/binary 等错误显示提示面板，Monaco 不挂载。
// monacoSetup 必须是首行 import：Monaco 求值前先装好 worker 工厂。
import '../monacoSetup'
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import MonacoEditor from 'monaco-editor-vue'
import { useDialog, useMessage } from 'naive-ui'
import { LoaderCircle, Save, TriangleAlert } from 'lucide-vue-next'
import type { OpenTab } from '../../../types/session'
import { classifyFsError, fsErrorMessage } from '../../../types/project'
import { useProjectStore } from '../../../stores/project'
import { useWorkspaceStore } from '../../../stores/workspace'
import { fsService } from '../../../services/fsService'
import { languageByPath } from '../../../utils/file'

const props = defineProps<{ tab: OpenTab }>()

const projectStore = useProjectStore()
const workspace = useWorkspaceStore()
const message = useMessage()
const dialog = useDialog()

const path = computed(() => props.tab.filePath ?? '')
const language = computed(() => languageByPath(path.value))
const rootPath = computed(() => projectStore.get(props.tab.projectId)?.rootPath ?? null)

const content = ref('')
const loading = ref(false)
/** 读取/保存错误（too-large、binary、not-found 等）：显示提示面板 */
const loadError = ref<string | null>(null)
const saving = ref(false)

/** dirty 基线（design D5）：加载/保存成功后的 mtime 与文本 */
const baselineMtime = ref<number | null>(null)
const baselineText = ref('')

const dirty = computed(() => !loadError.value && content.value !== baselineText.value)

/** 同步 dirty 状态到 workspace（标签圆点） */
watch(
  [dirty, () => props.tab.id],
  () => workspace.setFileDirty(props.tab.id, dirty.value),
  { immediate: true },
)

async function load(): Promise<void> {
  const root = rootPath.value
  if (!root || !path.value) {
    loadError.value = '文件标签缺少项目或路径信息'
    return
  }
  loading.value = true
  loadError.value = null
  try {
    const result = await fsService.readFile(path.value, root)
    content.value = result.text
    baselineText.value = result.text
    baselineMtime.value = result.mtimeMs
    workspace.setFileDirty(props.tab.id, false)
  } catch (err) {
    content.value = ''
    baselineText.value = ''
    baselineMtime.value = null
    loadError.value = fsErrorMessage(err)
    workspace.setFileDirty(props.tab.id, false)
  } finally {
    loading.value = false
  }
}

/** 覆盖保存（跳过冲突校验：读最新 mtime 作为 expected） */
async function forceSave(): Promise<void> {
  const root = rootPath.value
  if (!root || !path.value) return
  saving.value = true
  try {
    // 读取磁盘当前 mtime 作为新的 expected（外部修改已确认接受）
    let expected: number | null = null
    try {
      const current = await fsService.readFile(path.value, root)
      expected = current.mtimeMs
    } catch {
      expected = null // 文件可能已被外部删除：expected null 走首次创建分支
    }
    const result = await fsService.writeFile(path.value, content.value, expected, root)
    baselineText.value = content.value
    baselineMtime.value = result.mtimeMs
    workspace.setFileDirty(props.tab.id, false)
    message.success('已保存')
  } catch (err) {
    message.error(`保存失败：${fsErrorMessage(err)}`)
  } finally {
    saving.value = false
  }
}

/** 冲突处理：重新加载 / 覆盖 二选一（R5，不静默覆盖） */
function showConflictDialog(): void {
  dialog.warning({
    title: '文件已被外部修改',
    content: '磁盘上的内容与加载时不一致。选择「重新加载」放弃本地修改，或「覆盖」以当前编辑内容写入。',
    positiveText: '重新加载',
    negativeText: '覆盖',
    maskClosable: false,
    onPositiveClick: () => void load(),
    onNegativeClick: () => void forceSave(),
  })
}

async function save(): Promise<void> {
  const root = rootPath.value
  if (!root || !path.value || saving.value || loading.value || loadError.value) return
  saving.value = true
  try {
    const result = await fsService.writeFile(path.value, content.value, baselineMtime.value, root)
    baselineText.value = content.value
    baselineMtime.value = result.mtimeMs
    workspace.setFileDirty(props.tab.id, false)
    message.success('已保存')
  } catch (err) {
    if (classifyFsError(err) === 'conflict') showConflictDialog()
    else message.error(`保存失败：${fsErrorMessage(err)}`)
  } finally {
    saving.value = false
  }
}

function onKeydown(e: KeyboardEvent): void {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
    e.preventDefault()
    void save()
  }
}

function onChange(value: string): void {
  content.value = value
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  void load()
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  workspace.setFileDirty(props.tab.id, false)
})

// 同一组件实例被复用于不同标签时（tab.id 变化）重新加载
watch(
  () => props.tab.id,
  () => void load(),
)
</script>

<template>
  <div class="flex h-full flex-col bg-white">
    <!-- 工具行：路径 + dirty 状态 + 保存按钮 -->
    <div class="flex h-9 shrink-0 items-center gap-2 border-b border-neutral-200 px-3">
      <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-neutral-400" :title="path">
        {{ path }}
      </span>
      <span v-if="dirty" class="shrink-0 text-[11px] text-amber-500">未保存</span>
      <button
        class="flex h-6.5 shrink-0 cursor-pointer items-center gap-1.5 rounded-md border border-neutral-200 bg-white px-2.5 text-xs text-neutral-600 hover:bg-neutral-50 disabled:cursor-not-allowed disabled:opacity-40"
        :disabled="!dirty || saving || loading || !!loadError"
        title="保存（Ctrl+S）"
        @click="save"
      >
        <LoaderCircle v-if="saving" class="h-3.5 w-3.5 animate-spin text-blue-500" />
        <Save v-else class="h-3.5 w-3.5" />
        保存
      </button>
    </div>

    <!-- 读取中 -->
    <div v-if="loading" class="flex flex-1 items-center justify-center gap-2 text-xs text-neutral-400">
      <LoaderCircle class="h-4 w-4 animate-spin text-blue-500" />
      加载中…
    </div>

    <!-- 读取失败（too-large / binary / not-found 等）：提示面板，Monaco 不挂载 -->
    <div v-else-if="loadError" class="flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
      <TriangleAlert class="h-6 w-6 text-amber-400" />
      <p class="text-xs text-neutral-500">{{ loadError }}</p>
      <button
        class="cursor-pointer rounded-md border border-neutral-200 bg-white px-3 py-1 text-xs text-neutral-600 hover:bg-neutral-50"
        @click="load"
      >
        重试
      </button>
    </div>

    <MonacoEditor
      v-else
      :model-value="content"
      :language="language"
      theme="vs"
      height="100%"
      width="100%"
      :options="{
        minimap: { enabled: false },
        fontSize: 13,
        tabSize: 2,
        wordWrap: 'on',
        // 迷你灰色滚动条：窄、无阴影、半透明滑块
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
      @change="onChange"
    />
  </div>
</template>
