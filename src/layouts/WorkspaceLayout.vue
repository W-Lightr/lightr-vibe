<script setup lang="ts">
// 路由布局壳：只放一个子路由出口。
//
// RouterView 显式 import —— 虽然 `app.use(router)` 会全局注册它，
// 但隐式依赖全局注册在编译期没有类型保障，显式写出来更可靠。
import { RouterView } from 'vue-router'
</script>

<template>
  <RouterView />
</template>
