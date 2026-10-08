<script setup lang="ts">
// OpenCode 详情容器：头部（图标/名称/binary 徽章/描述/启用开关）+ 预检面板 +
// 配置卡片编排；未安装/检测失败显示安装引导分支，浏览器 mock 显示提示条（PRD R3）。
import { computed } from 'vue'
import { useMessage } from 'naive-ui'
import { Info, Terminal } from 'lucide-vue-next'
// lobehub opencode 图标（currentColor 内联 SVG）
import opencodeIcon from '@lobehub/icons-static-svg/icons/opencode.svg?raw'
import { useAgentStore } from '../../../stores/agent'
import PrecheckPanel from './PrecheckPanel.vue'
import ModelConfigCard from './ModelConfigCard.vue'
import PermissionCard from './PermissionCard.vue'
import EnvVarsCard from './EnvVarsCard.vue'

const agentStore = useAgentStore()
const message = useMessage()

const detect = computed(() => agentStore.detectResult)
const notInstalled = computed(() => !!detect.value && !detect.value.installed)

/** 配置编辑区可见：已安装且配置文件可读（未安装/无文件走引导分支，PRD R3） */
const configReady = computed(
  () => !!detect.value?.installed && !!detect.value.configExists && !!agentStore.configView,
)

/** 启用开关：本期仅记录到 store，不影响会话链路（design D5） */
function onToggleEnabled() {
  agentStore.enabled = !agentStore.enabled
  message.info(agentStore.enabled ? 'OpenCode 已启用（本期仅记录设置）' : 'OpenCode 已停用（本期仅记录设置）')
}
</script>

<template>
  <div class="flex flex-col gap-3 p-4">
    <!-- 浏览器 dev 模式提示条（PRD R3 / AC7） -->
    <div
      v-if="agentStore.browserMode"
      class="flex items-center gap-1.5 rounded-md border border-amber-400/40 bg-amber-400/10 px-3 py-1.5 text-xs text-neutral-600"
    >
      <Info class="h-3.5 w-3.5 shrink-0 text-amber-400" />
      浏览器模式：数据为 mock，仅用于界面开发；真实检测与配置读写需在桌面端运行。
    </div>

    <!-- 详情头 -->
    <div class="flex items-center gap-3 rounded-md border border-neutral-200 bg-white p-3">
      <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-md bg-neutral-100 text-neutral-800 [&>svg]:h-6 [&>svg]:w-6" v-html="opencodeIcon" />
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2">
          <span class="text-[13px] font-medium text-neutral-800">OpenCode</span>
          <span class="rounded-md bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">binary</span>
          <span v-if="detect?.version" class="rounded-md bg-neutral-100 px-1.5 py-0.5 font-mono text-[10px] text-neutral-500">
            v{{ detect.version }}
          </span>
        </div>
        <p class="mt-0.5 truncate text-xs text-neutral-500">
          支持 ACP（Agent Client Protocol）的开源编码 Agent；配置读取自 opencode.jsonc。
        </p>
      </div>
      <!-- 启用开关：本期仅记录（design D5） -->
      <button
        class="relative h-5 w-9 shrink-0 cursor-pointer rounded-full transition-colors"
        :class="agentStore.enabled ? 'bg-blue-500' : 'bg-neutral-300'"
        :title="agentStore.enabled ? '停用 OpenCode' : '启用 OpenCode'"
        role="switch"
        :aria-checked="agentStore.enabled"
        @click="onToggleEnabled"
      >
        <span
          class="absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-white transition-transform"
          :style="{ transform: agentStore.enabled ? 'translateX(16px)' : 'none' }"
        />
      </button>
    </div>

    <!-- 未检测：占位空态 -->
    <div
      v-if="!detect && !agentStore.detecting"
      class="rounded-md border border-dashed border-neutral-200 bg-white px-3 py-6 text-center text-xs text-neutral-400"
    >
      尚未检测，点击预检面板的「诊断」开始。
    </div>

    <!-- 未安装引导（PRD R3：不显示配置编辑区） -->
    <div v-else-if="notInstalled" class="rounded-md border border-neutral-200 bg-white p-4">
      <div class="flex items-center gap-2 text-[13px] font-medium text-neutral-800">
        <Terminal class="h-4 w-4 text-neutral-500" />
        未检测到 OpenCode
      </div>
      <p class="mt-2 text-xs text-neutral-600">请先安装 OpenCode 并确保其在 PATH 中，再回到本页重新检测：</p>
      <pre class="mt-2 rounded-md bg-neutral-100 p-2 font-mono text-[11px] text-neutral-600">npm install -g opencode-ai</pre>
      <p v-if="detect?.detail" class="mt-2 text-[11px] text-neutral-400">检测明细：{{ detect.detail }}</p>
    </div>

    <template v-else>
      <!-- 预检面板 -->
      <PrecheckPanel />

      <!-- 配置文件缺失/解析失败：引导分支（检测进行中不渲染，避免空盒闪现） -->
      <div
        v-if="!configReady && !agentStore.detecting"
        class="rounded-md border border-neutral-200 bg-white p-4 text-xs text-neutral-600"
      >
        <template v-if="agentStore.configError">
          配置文件解析失败：{{ agentStore.configError }}
          <span class="mt-1 block font-mono text-[11px] text-neutral-400">{{ agentStore.configPath }}</span>
        </template>
        <template v-else-if="detect && !detect.configExists">
          配置文件不存在，可手动创建后刷新：
          <span class="mt-1 block font-mono text-[11px] text-neutral-400">{{ detect.configPath }}</span>
        </template>
      </div>

      <!-- 配置编辑区（已安装 + 配置可读才展示，PRD R3/R4/R5/R6） -->
      <template v-else>
        <ModelConfigCard />
        <PermissionCard />
      </template>

      <!-- 环境变量：与 opencode.jsonc 无关（应用级文件），已安装即可编辑（PRD R6） -->
      <EnvVarsCard />
    </template>
  </div>
</template>
