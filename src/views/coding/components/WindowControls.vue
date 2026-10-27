<script setup lang="ts">
// 窗口三键（最小化 / 最大化·还原 / 关闭）。
//
// 抽成组件是为了让「任务看板」「自动化」这类非编码页共用同一套外观与行为：
// 之前每个页面各写一遍，重做界面时漏掉一处就会出现「没有关闭按钮」的窗口
// （无边框窗口只能靠 Alt+F4，非常别扭）。
//
// 浏览器 dev 环境 getAppWindow() 返回 null，整个组件不渲染（无边框窗口不存在）。
//
// `compact` 供侧栏使用：侧栏最小宽度 200px，旁边还有 4 个 28px 的工具按钮，
// 全尺寸（3×32px）会溢出，所以侧栏用 24px 的紧凑版。
import { Square, Minus, X } from 'lucide-vue-next'
import { getAppWindow } from '../../../utils/window'

const props = defineProps<{
  /** 紧凑模式：侧栏等窄容器里使用 */
  compact?: boolean
}>()

const appWindow = getAppWindow()

const btnClass = () =>
  props.compact
    ? 'flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-200/70'
    : 'flex h-8 w-8 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-100'

const closeClass = () =>
  props.compact
    ? 'flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-red-500 hover:text-white'
    : 'flex h-8 w-8 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-red-500 hover:text-white'
</script>

<template>
  <div v-if="appWindow" class="flex shrink-0 items-center gap-0.5" data-no-drag>
    <button :class="btnClass()" title="最小化" @click="appWindow.minimize()">
      <Minus :class="compact ? 'h-3 w-3' : 'h-4 w-4'" />
    </button>
    <button :class="btnClass()" title="最大化 / 还原" @click="appWindow.toggleMaximize()">
      <Square :class="compact ? 'h-2.5 w-2.5' : 'h-3.5 w-3.5'" />
    </button>
    <button :class="closeClass()" title="关闭" @click="appWindow.close()">
      <X :class="compact ? 'h-3 w-3' : 'h-4 w-4'" />
    </button>
  </div>
</template>
