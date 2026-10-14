<script setup lang="ts">
// 权限审批停靠条：贴在 composer 上方（照 codeg：权限卡在输入区上方的 dock，不随消息流滚动）。
// 按 connId（= 本地会话 id）过滤本会话待审批请求；confirm 模式下列出（通常一次一张）。
import { computed } from 'vue'
import { useAcpPermission } from '../../../composables/useAcpPermission'
import PermissionCard from './PermissionCard.vue'

const props = defineProps<{ sessionId: string }>()

const { pending } = useAcpPermission()
const permissionQueue = computed(() => pending.value.filter((e) => e.connId === props.sessionId))
</script>

<template>
  <div v-if="permissionQueue.length > 0" class="px-6 pb-1">
    <div class="mx-auto max-w-3xl">
      <PermissionCard
        v-for="(req, i) in permissionQueue"
        :key="req.requestId"
        :event="req"
        :queued-count="permissionQueue.length - 1 - i"
      />
    </div>
  </div>
</template>
