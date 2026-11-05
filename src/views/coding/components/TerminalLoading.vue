<script setup lang="ts">
// 终端 chunk（xterm）加载中的占位：深色底与终端主题一致，不依赖 xterm 自身 CSS。
import { LoaderCircle } from 'lucide-vue-next'
</script>

<template>
  <div class="flex h-full w-full items-center justify-center gap-2 bg-neutral-900 text-xs text-neutral-400">
    <LoaderCircle class="h-4 w-4 animate-spin text-blue-400" />
    加载终端…
  </div>
</template>
