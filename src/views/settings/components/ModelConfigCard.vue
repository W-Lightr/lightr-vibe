<script setup lang="ts">
// 模型与 Provider 配置卡片（PRD R4）：
// 主/小模型下拉（选项 = providers × models 的 provider/model）→ 写回顶层 model / small_model；
// Provider 列表（名称/npm/baseURL/模型数/apiKey 掩码+显示切换+编辑保存）；
// 「刷新目录」重新读盘；「连接 Provider」本期占位提示（避免误写用户凭据）。
import { computed, ref, watch } from 'vue'
import { NSelect, useMessage } from 'naive-ui'
import { Eye, EyeOff, Pencil, Plug, RefreshCw, Check, X } from 'lucide-vue-next'
import { useAgentStore } from '../../../stores/agent'
import type { ProviderView } from '../../../types/agent'

const agentStore = useAgentStore()
const message = useMessage()

/** 下拉选项：provider/model-id（顶层 model 字段的合法值格式） */
const modelOptions = computed(() => {
  const options: { label: string; value: string }[] = []
  for (const provider of agentStore.configView?.providers ?? []) {
    for (const model of provider.models) {
      options.push({ label: model.label, value: model.label })
    }
  }
  return options
})

// ---- 主/小模型：本地草稿 + 变更即保存（失败回滚） ----
const modelDraft = ref<string | null>(null)
const smallModelDraft = ref<string | null>(null)

watch(
  () => [agentStore.configView?.model, agentStore.configView?.smallModel],
  ([model, small]) => {
    modelDraft.value = model ?? null
    smallModelDraft.value = small ?? null
  },
  { immediate: true },
)

async function onModelChange(kind: 'model' | 'small', value: string | null) {
  const prev = kind === 'model' ? agentStore.configView?.model : agentStore.configView?.smallModel
  const err = await agentStore.saveModel(kind, value)
  if (err) {
    // 失败回滚草稿并保留编辑能力（PRD R7：写失败保留编辑态并报错）
    if (kind === 'model') modelDraft.value = prev ?? null
    else smallModelDraft.value = prev ?? null
    reportError(err.kind === 'conflict' ? '配置已被外部修改，请刷新目录后重试' : err.message, err.kind)
  } else {
    message.success(kind === 'model' ? '主模型已保存' : '小模型已保存')
  }
}

function reportError(text: string, kind: 'conflict' | 'error') {
  if (kind === 'conflict') {
    message.error(text)
    return
  }
  message.error(`保存失败：${text}`)
}

// ---- Provider apiKey：掩码显示 / 明文切换 / 行内编辑保存 ----
const revealKeys = ref<Record<string, boolean>>({})
const editingProvider = ref<string | null>(null)
const apiKeyDraft = ref('')

function toggleReveal(name: string) {
  revealKeys.value[name] = !revealKeys.value[name]
}

function startEdit(provider: ProviderView) {
  editingProvider.value = provider.key
  apiKeyDraft.value = provider.apiKeyPlain ?? ''
}

function cancelEdit() {
  editingProvider.value = null
  apiKeyDraft.value = ''
}

async function commitEdit(provider: ProviderView) {
  // 补丁路径用配置对象键 provider.key（显示名 name 可能与键不同，如中文 provider）
  const err = await agentStore.saveApiKey(provider.key, apiKeyDraft.value)
  if (err) {
    reportError(
      err.kind === 'conflict' ? '配置已被外部修改，请刷新目录后重试' : err.message,
      err.kind,
    )
    return
  }
  editingProvider.value = null
  apiKeyDraft.value = ''
  message.success('apiKey 已保存')
}

async function refreshCatalog() {
  try {
    await agentStore.loadConfig()
    message.success('目录已刷新')
  } catch (e) {
    message.error(`刷新失败：${e instanceof Error ? e.message : String(e)}`)
  }
}

function connectProvider() {
  // 本期占位：不写用户凭据，提示手动编辑路径（PRD R4）
  message.info(`本期请手动编辑配置文件添加 Provider：${agentStore.configPath}`)
}
</script>

<template>
  <section class="rounded-md border border-neutral-200 bg-white">
    <div class="flex items-center justify-between border-b border-neutral-200 px-3 py-2">
      <span class="text-xs font-medium text-neutral-600">模型与 Provider</span>
      <div class="flex items-center gap-1.5">
        <button
          class="flex cursor-pointer items-center gap-1 rounded-md border border-neutral-300 px-2 py-1 text-xs text-neutral-600 transition-colors hover:bg-neutral-100"
          title="重新读取配置文件"
          @click="refreshCatalog"
        >
          <RefreshCw class="h-3.5 w-3.5" />
          刷新目录
        </button>
        <button
          class="flex cursor-pointer items-center gap-1 rounded-md bg-blue-500 px-2 py-1 text-xs text-white transition-colors hover:bg-blue-600"
          title="添加新的 Provider（本期占位）"
          @click="connectProvider"
        >
          <Plug class="h-3.5 w-3.5" />
          连接 Provider
        </button>
      </div>
    </div>

    <div class="flex flex-col gap-3 p-3">
      <!-- 主模型 / 小模型（small_model 为官方 snake_case 键，research §3） -->
      <div class="grid grid-cols-2 gap-3">
        <label class="flex flex-col gap-1">
          <span class="text-[11px] text-neutral-500">主模型（model）</span>
          <NSelect
            v-model:value="modelDraft"
            :options="modelOptions"
            size="small"
            clearable
            :disabled="agentStore.writing"
            placeholder="provider/model-id"
            @update:value="(v: string | null) => onModelChange('model', v)"
          />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-[11px] text-neutral-500">小模型（small_model）</span>
          <NSelect
            v-model:value="smallModelDraft"
            :options="modelOptions"
            size="small"
            clearable
            :disabled="agentStore.writing"
            placeholder="provider/model-id"
            @update:value="(v: string | null) => onModelChange('small', v)"
          />
        </label>
      </div>

      <!-- Provider 列表 -->
      <div class="flex flex-col gap-1.5">
        <span class="text-[11px] text-neutral-500">
          已配置 Provider：{{ agentStore.configView?.providers.length ?? 0 }}
        </span>
        <div
          v-for="provider in agentStore.configView?.providers ?? []"
          :key="provider.key"
          class="rounded-md border border-neutral-200 p-2"
        >
          <div class="flex items-center gap-2">
            <span class="min-w-0 flex-1 truncate text-[13px] font-medium text-neutral-800">
              {{ provider.name }}
            </span>
            <span class="shrink-0 rounded-md bg-neutral-100 px-1.5 py-0.5 text-[10px] text-neutral-500">
              {{ provider.models.length }} 个模型
            </span>
          </div>
          <div class="mt-1 flex flex-col gap-0.5 text-[11px] text-neutral-500">
            <span class="truncate font-mono">{{ provider.npm || '（未声明 npm SDK）' }}</span>
            <span class="truncate font-mono">{{ provider.baseURL || '（未声明 baseURL）' }}</span>
            <span v-if="provider.whitelist.length" class="truncate">
              白名单：{{ provider.whitelist.join('、') }}
            </span>
          </div>

          <!-- apiKey：默认掩码，显示切换 + 行内编辑（PRD R4/R7） -->
          <div class="mt-1.5 flex items-center gap-1.5">
            <span class="text-[11px] text-neutral-400">apiKey</span>
            <template v-if="editingProvider === provider.key">
              <input
                v-model="apiKeyDraft"
                class="h-6 min-w-0 flex-1 rounded-md border border-neutral-300 px-1.5 font-mono text-[11px] outline-none focus:border-blue-400"
                placeholder="输入新的 apiKey"
                data-no-drag
              />
              <button
                class="flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-green-600 hover:bg-neutral-100"
                title="保存"
                :disabled="agentStore.writing"
                @click="commitEdit(provider)"
              >
                <Check class="h-3.5 w-3.5" />
              </button>
              <button
                class="flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-neutral-500 hover:bg-neutral-100"
                title="取消"
                @click="cancelEdit"
              >
                <X class="h-3.5 w-3.5" />
              </button>
            </template>
            <template v-else>
              <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-neutral-600">
                {{ revealKeys[provider.key] ? (provider.apiKeyPlain ?? '（未设置）') : (provider.apiKeyMasked ?? '（未设置）') }}
              </span>
              <button
                v-if="provider.apiKeyPlain"
                class="flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-neutral-400 hover:bg-neutral-100"
                :title="revealKeys[provider.key] ? '隐藏' : '显示'"
                @click="toggleReveal(provider.key)"
              >
                <EyeOff v-if="revealKeys[provider.key]" class="h-3.5 w-3.5" />
                <Eye v-else class="h-3.5 w-3.5" />
              </button>
              <button
                class="flex h-6 w-6 cursor-pointer items-center justify-center rounded-md text-neutral-400 hover:bg-neutral-100"
                title="编辑 apiKey"
                @click="startEdit(provider)"
              >
                <Pencil class="h-3.5 w-3.5" />
              </button>
            </template>
          </div>
        </div>
        <div
          v-if="(agentStore.configView?.providers.length ?? 0) === 0"
          class="rounded-md border border-dashed border-neutral-200 px-3 py-4 text-center text-xs text-neutral-400"
        >
          配置文件中暂无 Provider
        </div>
      </div>
    </div>
  </section>
</template>
