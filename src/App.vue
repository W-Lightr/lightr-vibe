<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue'
import { NConfigProvider, NMessageProvider } from 'naive-ui'
import { getAppWindow } from './utils/window'

// ---- 无边框窗口：全局拖拽 / 双击最大化 ----
// 拖拽区域由 [data-drag-region] 标记；交互控件与 [data-no-drag] 区域排除在外
// 浏览器 dev 环境 appWindow 为 null：拖拽逻辑自动停用（AC7）
const appWindow = getAppWindow()

const INTERACTIVE = 'button, a, input, textarea, select, [data-no-drag]'

function dragTarget(e: MouseEvent): HTMLElement | null {
  if (e.button !== 0) return null
  const target = e.target as HTMLElement
  if (target.closest(INTERACTIVE)) return null
  return target.closest('[data-drag-region]')
}

function onMouseDown(e: MouseEvent) {
  if (appWindow && dragTarget(e)) void appWindow.startDragging()
}

function onDblClick(e: MouseEvent) {
  if (appWindow && dragTarget(e)) void appWindow.toggleMaximize()
}

onMounted(() => {
  window.addEventListener('mousedown', onMouseDown)
  window.addEventListener('dblclick', onDblClick)
})
onBeforeUnmount(() => {
  window.removeEventListener('mousedown', onMouseDown)
  window.removeEventListener('dblclick', onDblClick)
})
</script>

<template>
  <NConfigProvider class="h-full">
    <NMessageProvider>
      <div class="h-screen overflow-hidden bg-white text-neutral-800">
        <RouterView />
      </div>
    </NMessageProvider>
  </NConfigProvider>
</template>
