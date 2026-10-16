<script setup lang="ts">
import { computed } from 'vue'
import { marked } from 'marked'

const props = defineProps<{ content: string }>()

// 轻量 Markdown 渲染：代码块 / 列表 / 引用 / 表格 / 标题
const html = computed(() =>
  marked.parse(props.content, { async: false, breaks: true }) as string,
)

// 链接外部打开由 main.ts 的全局 capture 监听统一处理（避免本地+全局重复触发开两次）。
</script>

<template>
  <div class="md-body" v-html="html" />
</template>

<style>
.md-body {
  font-size: 13.5px;
  line-height: 1.75;
  color: #262626;
  word-break: break-word;
}
.md-body > *:first-child {
  margin-top: 0;
}
.md-body > *:last-child {
  margin-bottom: 0;
}
.md-body h1,
.md-body h2,
.md-body h3 {
  margin: 12px 0 6px;
  font-size: 14px;
  font-weight: 600;
  color: #171717;
}
.md-body p {
  margin: 6px 0;
}
.md-body ul,
.md-body ol {
  margin: 6px 0;
  padding-left: 20px;
}
.md-body li {
  margin: 2px 0;
}
.md-body blockquote {
  margin: 8px 0;
  padding: 4px 12px;
  border-left: 3px solid #d4d4d4;
  color: #737373;
  background: #fafafa;
  border-radius: 0 4px 4px 0;
}
.md-body code {
  padding: 1px 5px;
  border-radius: 4px;
  background: #f5f5f5;
  border: 1px solid #e5e5e5;
  font-family: Consolas, 'Courier New', monospace;
  font-size: 12.5px;
  word-break: break-all;
}
.md-body pre {
  margin: 8px 0;
  padding: 10px 12px;
  border-radius: 6px;
  background: #171717;
  overflow-x: auto;
}
.md-body pre code {
  padding: 0;
  border: none;
  background: transparent;
  color: #e5e5e5;
  font-size: 12.5px;
  word-break: normal;
}
.md-body table {
  margin: 8px 0;
  border-collapse: collapse;
  font-size: 12.5px;
}
.md-body th,
.md-body td {
  padding: 4px 12px;
  border: 1px solid #e5e5e5;
}
.md-body th {
  background: #fafafa;
  font-weight: 600;
}
.md-body a {
  color: #2563eb;
  word-break: break-all;
}
</style>
