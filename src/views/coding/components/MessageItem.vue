<script setup lang="ts">
// 用户消息气泡。
//
// 原本这是一个「万能消息组件」：用户气泡 / 思考 / 错误 / assistant 正文全在这里分支。
// 对话界面改为 part 驱动渲染后（见 AssistantTurn.vue），assistant 侧不再经过本组件，
// 它只剩「用户消息」一种用途 —— 于是把其余分支与随之失效的依赖一并移除。
import { computed } from 'vue'
import type { Message } from '../../../types/session'

const props = defineProps<{ message: Message }>()

const isUser = computed(() => props.message.role === 'user')
</script>

<template>
  <div v-if="isUser" class="mb-5 flex justify-end">
    <div
      class="max-w-[75%] whitespace-pre-wrap break-words rounded-2xl rounded-br-md bg-neutral-100 px-4 py-2.5 text-[13.5px] leading-6 text-neutral-800"
    >
      {{ message.content }}
    </div>
  </div>
</template>
