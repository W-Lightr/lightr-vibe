<script setup lang="ts">
// 窗口三键（最小化 / 最大化·还原 / 关闭）。
//
// 无边框窗口的必需控件。放在每个页面 header 的最右端 —— 整个窗口只有这一处，
// 不要在多处重复渲染（侧栏曾经也有一份紧凑版，已移除：同一屏出现两组窗口按钮
// 是纯噪音）。
//
// 浏览器 dev 环境 getAppWindow() 返回 null，整个组件不渲染。
import { Square, Minus, X } from 'lucide-vue-next'
import { getAppWindow } from '../../../utils/window'

const appWindow = getAppWindow()
</script>

<template>
  <div v-if="appWindow" class="flex shrink-0 items-center gap-0.5" data-no-drag>
    <button
      class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-100"
      title="最小化"
      @click="appWindow.minimize()"
    >
      <Minus class="h-4 w-4" />
    </button>
    <button
      class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-100"
      title="最大化 / 还原"
      @click="appWindow.toggleMaximize()"
    >
      <Square class="h-3.5 w-3.5" />
    </button>
    <button
      class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-red-500 hover:text-white"
      title="关闭"
      @click="appWindow.close()"
    >
      <X class="h-4 w-4" />
    </button>
  </div>
</template>
