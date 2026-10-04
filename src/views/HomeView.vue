<script setup lang="ts">
import { useRouter } from 'vue-router'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { Code, Minus, Square, X } from 'lucide-vue-next'

const router = useRouter()
const appWindow = getCurrentWindow()

// 配置数组驱动渲染，后续新功能只需在此追加
const features = [
  {
    id: 'coding',
    name: '编码',
    icon: Code,
    route: '/coding',
    description: 'AI 辅助编码工作区',
  },
]
</script>

<template>
  <div class="relative flex h-full flex-col items-center justify-center gap-12 px-6">
    <!-- 无边框窗口拖拽区（顶部留白条）+ 窗口控制 -->
    <div data-drag-region class="absolute inset-x-0 top-0 flex h-10 items-center justify-end px-2">
      <div class="flex items-center gap-0.5" data-no-drag>
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
    </div>

    <header class="text-center">
      <h1 class="text-3xl font-semibold tracking-wide text-neutral-800">Lightr Vibe</h1>
      <p class="mt-2 text-sm text-neutral-500">选择一个功能开始</p>
    </header>

    <div class="grid w-full max-w-3xl grid-cols-1 gap-6 sm:grid-cols-2">
      <button
        v-for="feature in features"
        :key="feature.id"
        class="group flex cursor-pointer flex-col items-start gap-3 rounded-xl border border-neutral-200 bg-white p-6 text-left shadow-sm transition-colors hover:border-blue-300 hover:shadow-md"
        @click="router.push(feature.route)"
      >
        <span
          class="flex h-12 w-12 items-center justify-center rounded-lg bg-blue-50 text-blue-600 transition-transform group-hover:scale-105"
        >
          <component :is="feature.icon" class="h-6 w-6" />
        </span>
        <span class="text-lg font-medium text-neutral-800">{{ feature.name }}</span>
        <span class="text-xs text-neutral-500">{{ feature.description }}</span>
      </button>
    </div>
  </div>
</template>
