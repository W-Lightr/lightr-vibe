<script setup lang="ts">
import { useRouter } from 'vue-router'
import { useMessage } from 'naive-ui'
import type { Component } from 'vue'
import { Code, Minus, Settings, Square, X } from 'lucide-vue-next'
import { getAppWindow } from '../utils/window'
import { openSettingsWindow } from '../utils/settingsWindow'

const router = useRouter()
const message = useMessage()
// 浏览器 dev 环境为 null：隐藏窗口三键（AC7）
const appWindow = getAppWindow()

/** 桌面端打开独立设置窗口；浏览器 dev 回退页内 /settings */
async function openSettings(): Promise<void> {
  try {
    if (await openSettingsWindow()) return
  } catch (e) {
    message.error(`打开设置窗口失败：${e instanceof Error ? e.message : String(e)}`)
    return
  }
  void router.push('/settings')
}

interface FeatureEntry {
  id: string
  name: string
  icon: Component
  route: string
  description: string
  /**
   * 可选的入口预热：悬停/聚焦时提前加载目标路由外壳 chunk。
   * 只允许预热路由外壳，重依赖（编辑器/终端/画布）必须留在目标功能的异步子模块里。
   */
  prefetch?: () => void
}

// 配置数组驱动渲染，后续新功能只需在此追加
const features: FeatureEntry[] = [
  {
    id: 'coding',
    name: '编码',
    icon: Code,
    route: '/coding',
    description: 'AI 辅助编码工作区',
    prefetch: () => {
      // 必须显式取 default（与路由处一致）：直接用整段模块命名空间会让打包器生成
      // 命名空间包装，其 helper 目前被分到 Monaco chunk，反把 editor.api 拉回静态依赖。
      void import('../views/coding/CodingWorkspace.vue')
        .then((m) => m.default)
        .catch(() => undefined)
    },
  },
]
</script>

<template>
  <div class="relative flex h-full flex-col items-center justify-center gap-12 px-6">
    <!-- 无边框窗口拖拽区（顶部留白条）+ 窗口控制 -->
    <div data-drag-region class="absolute inset-x-0 top-0 flex h-10 items-center justify-end px-2">
      <div class="flex items-center gap-0.5" data-no-drag>
        <!-- 设置入口（PRD R1：Home 右上角齿轮） -->
        <button
          class="mr-1 flex h-8 w-8 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-100"
          title="设置"
          @click="openSettings"
        >
          <Settings class="h-4 w-4" />
        </button>
        <button
          class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-100"
          title="最小化"
          @click="appWindow?.minimize()"
        >
          <Minus class="h-4 w-4" />
        </button>
        <button
          class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-100"
          title="最大化 / 还原"
          @click="appWindow?.toggleMaximize()"
        >
          <Square class="h-3.5 w-3.5" />
        </button>
        <button
          class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-red-500 hover:text-white"
          title="关闭"
          @click="appWindow?.close()"
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
        @pointerenter="feature.prefetch?.()"
        @focus="feature.prefetch?.()"
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
