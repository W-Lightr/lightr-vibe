<script setup lang="ts">
// 通用引擎详情（非 OpenCode 引擎）：头部（图标/名称/适配器徽章/描述）+ 预检面板。
// Claude Code / Codex 经官方 ACP 适配器启动，用户自装、本体不打包不下载（design D1）；
// 本页不做配置编辑（D-cfg），仅在未安装时给出 `npm i -g <package>` 安装指引。
import { computed } from 'vue'
import { Info, Terminal } from 'lucide-vue-next'
import { useAgentStore } from '../../../stores/agent'
import { useEngineStore } from '../../../stores/engine'
import type { EngineWire } from '../../../types/engine'
import PrecheckPanel from './PrecheckPanel.vue'

const props = defineProps<{
  /** 当前详情聚焦的引擎 wire */
  engineId: EngineWire
  /** 引擎图标内联 SVG（由父级映射，未知为空串） */
  iconSvg: string
}>()

const agentStore = useAgentStore()
const engineStore = useEngineStore()

const detect = computed(() => agentStore.detectResult)
const notInstalled = computed(() => !!detect.value && !detect.value.installed)
const displayName = computed(() => engineStore.agentName(props.engineId))

/** 适配器安装包规格（仅用于安装指引展示，不触发任何下载） */
const installPackage = computed(() =>
  props.engineId === 'codex'
    ? '@agentclientprotocol/codex-acp'
    : '@agentclientprotocol/claude-agent-acp',
)
</script>

<template>
  <div class="flex flex-col gap-3 p-4">
    <!-- 浏览器 dev 模式提示条（与 OpenCode 详情一致） -->
    <div
      v-if="agentStore.browserMode"
      class="flex items-center gap-1.5 rounded-md border border-amber-400/40 bg-amber-400/10 px-3 py-1.5 text-xs text-neutral-600"
    >
      <Info class="h-3.5 w-3.5 shrink-0 text-amber-400" />
      浏览器模式：数据为 mock，仅用于界面开发；真实检测与配置读写需在桌面端运行。
    </div>

    <!-- 详情头 -->
    <div class="flex items-center gap-3 rounded-md border border-neutral-200 bg-white p-3">
      <span
        class="flex h-10 w-10 shrink-0 items-center justify-center rounded-md bg-neutral-100 text-neutral-800 [&>svg]:h-6 [&>svg]:w-6"
        v-html="props.iconSvg"
      />
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2">
          <span class="text-[13px] font-medium text-neutral-800">{{ displayName }}</span>
          <span class="rounded-md bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">adapter</span>
          <span
            v-if="detect?.version"
            class="rounded-md bg-neutral-100 px-1.5 py-0.5 font-mono text-[10px] text-neutral-500"
          >
            v{{ detect.version }}
          </span>
        </div>
        <p class="mt-0.5 truncate text-xs text-neutral-500">
          经官方 ACP 适配器启动（共享厂商 CLI 凭据）；适配器需用户自行安装，本体不打包、不自动下载。
        </p>
      </div>
    </div>

    <!-- 未检测：占位空态 -->
    <div
      v-if="!detect && !agentStore.detecting"
      class="rounded-md border border-dashed border-neutral-200 bg-white px-3 py-6 text-center text-xs text-neutral-400"
    >
      尚未检测，点击预检面板的「诊断」开始。
    </div>

    <!-- 未安装引导（AC9：明确文案 + npm i -g 指引，非崩溃） -->
    <div v-else-if="notInstalled" class="rounded-md border border-neutral-200 bg-white p-4">
      <div class="flex items-center gap-2 text-[13px] font-medium text-neutral-800">
        <Terminal class="h-4 w-4 text-neutral-500" />
        未检测到 {{ displayName }} 的适配器
      </div>
      <p class="mt-2 text-xs text-neutral-600">
        请在终端安装官方 ACP 适配器（需要 Node.js），再回到本页重新检测：
      </p>
      <pre class="mt-2 rounded-md bg-neutral-100 p-2 font-mono text-[11px] text-neutral-600">npm i -g {{ installPackage }}</pre>
      <p v-if="detect?.detail" class="mt-2 text-[11px] text-neutral-400">检测明细：{{ detect.detail }}</p>
    </div>

    <!-- 已安装：预检面板（不做配置编辑，D-cfg） -->
    <template v-else>
      <PrecheckPanel />
      <div class="rounded-md border border-neutral-200 bg-white p-3 text-[11px] text-neutral-500">
        该引擎无独立可编辑配置文件；凭据由厂商 CLI（~/.claude / ~/.codex）托管。
      </div>
    </template>
  </div>
</template>
