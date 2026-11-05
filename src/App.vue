<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue'
import { NConfigProvider, NDialogProvider, NMessageProvider } from 'naive-ui'
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
  if (!appWindow || !dragTarget(e)) return
  // 双击拖拽区放大/还原：必须在 mousedown 里判断 click 次数。
  // 原生 startDragging() 会进入系统拖拽循环，后续 dblclick 事件不会到达。
  if (e.detail === 2) {
    void appWindow.toggleMaximize()
    return
  }
  void appWindow.startDragging()
}

onMounted(() => {
  window.addEventListener('mousedown', onMouseDown)
})
onBeforeUnmount(() => {
  window.removeEventListener('mousedown', onMouseDown)
})
</script>

<template>
  <NConfigProvider class="h-full">
    <NMessageProvider>
      <NDialogProvider>
        <div class="h-screen overflow-hidden bg-white text-neutral-800">
          <RouterView />
        </div>
      </NDialogProvider>
    </NMessageProvider>
  </NConfigProvider>
</template>
