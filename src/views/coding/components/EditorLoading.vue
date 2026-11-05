<script setup lang="ts">
// 编辑器 chunk（Monaco）加载中的占位：与编辑器同底色，加载完成前不出现白屏闪烁。
import { LoaderCircle } from 'lucide-vue-next'
</script>

<template>
  <div class="flex h-full w-full items-center justify-center gap-2 bg-white text-xs text-neutral-400">
    <LoaderCircle class="h-4 w-4 animate-spin text-blue-500" />
    加载编辑器…
  </div>
</template>
