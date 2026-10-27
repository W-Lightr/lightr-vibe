<script setup lang="ts">
// 工作区外壳：左侧导航 + 主内容区。
//
// 从 CodingWorkspace 里抽出来的一层，让 /automations、/tasks 这类「非会话页」
// 也能保留左侧菜单（设计图要求），而不必各自复制一份侧栏 + 拖拽逻辑。
//
// 主内容区必须是 `<RouterView />` 而不是 `<slot />` —— 它作为父路由挂在
// 路由表里，子路由只有 RouterView 才会被渲染（用 slot 会得到一片空白）。
//
// 保留的策略与 CodingWorkspace 一致：
// - 宽度由 workspace.leftWidth 驱动，可拖拽分隔条（200–480）
// - 窗口过窄时自动收起侧栏
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { RouterView } from 'vue-router'
import SidebarPanel from '../views/coding/components/SidebarPanel.vue'
import { useWorkspaceStore } from '../stores/workspace'

const workspace = useWorkspaceStore()

const dragging = ref(false)
let startX = 0
let startWidth = 0

const LEFT_MIN = 200
const LEFT_MAX = 480

/** 侧栏当前是否占位（宽度 0 时整列不参与布局） */
const sidebarOpen = computed(() => workspace.sidebarVisible)

function onPointerMove(e: PointerEvent) {
  const dx = e.clientX - startX
  workspace.leftWidth = Math.min(LEFT_MAX, Math.max(LEFT_MIN, startWidth + dx))
}

function stopDrag() {
  dragging.value = false
  window.removeEventListener('pointermove', onPointerMove)
  window.removeEventListener('pointerup', stopDrag)
}

function startDrag(e: PointerEvent) {
  dragging.value = true
  startX = e.clientX
  startWidth = workspace.leftWidth
  window.addEventListener('pointermove', onPointerMove)
  window.addEventListener('pointerup', stopDrag)
}

const NARROW = 1280
function onResize() {
  if (window.innerWidth < NARROW) workspace.sidebarVisible = false
}

onMounted(() => {
  onResize()
  window.addEventListener('resize', onResize)
})

onBeforeUnmount(() => {
  stopDrag()
  window.removeEventListener('resize', onResize)
})
</script>

<style scoped>
.col {
  transition: width 0.22s cubic-bezier(0.25, 0.8, 0.25, 1);
  overflow: hidden;
  flex-shrink: 0;
}
.resizing .col {
  transition: none;
}

.divider {
  position: relative;
  z-index: 10;
  width: 1px;
  flex-shrink: 0;
  cursor: col-resize;
  background: transparent;
}
/* 不可见的加宽命中区（6px），不占布局宽度 */
.divider::after {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  left: -3px;
  right: -3px;
}
</style>

<template>
  <div class="flex h-full bg-white" :class="dragging && 'resizing'">
    <!-- 左侧菜单：与编码工作区共用同一个 SidebarPanel -->
    <div class="col" :style="{ width: (sidebarOpen ? workspace.leftWidth : 0) + 'px' }">
      <div class="h-full" :style="{ width: workspace.leftWidth + 'px' }">
        <SidebarPanel />
      </div>
    </div>
    <div v-if="sidebarOpen" class="divider" title="拖动调整侧栏宽度" @pointerdown="startDrag" />

    <!--
      主内容区：子路由出口。

      `relative` 是给页面内的全屏弹层用的：自动化编辑器 / 计划生成器 / 运行历史
      都用 `absolute inset-0` 覆盖「这一块」而不是整个视口，这样左侧菜单始终
      可见（设计图要求所有自动化界面都保留左侧菜单）。
    -->
    <div class="relative flex min-h-0 min-w-0 flex-1 flex-col">
      <RouterView />
    </div>
  </div>
</template>
