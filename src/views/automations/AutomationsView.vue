<script setup lang="ts">
// 自动化页（/automations）：模板库式列表 + 新建。
//
// 布局对齐设计图：
// - 顶部居中：闪电图标 + 标题 + 说明
// - 卡片网格：「空白自动化」虚线卡固定第一格，其余为已保存的自动化
// - 每张卡：彩色圆角图标块 + 名称 + 说明 + 触发方式胶囊
//
// 数据仍来自 automationStore（唯一副本）；变更事件订阅 `automation://changed`
// 后整表重读（调度器在后台自己触发，这是页面能自己更新的唯一途径）。
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useDialog, useMessage } from 'naive-ui'
import type { UnlistenFn } from '@tauri-apps/api/event'
import {
  ArrowLeft,
  Beaker,
  FileCode2,
  History,
  ListChecks,
  LoaderCircle,
  Package,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Rocket,
  Shield,
  Square,
  Trash2,
  Wrench,
  Zap,
} from 'lucide-vue-next'
import WindowControls from '../coding/components/WindowControls.vue'
import AutomationEditorDialog from './components/AutomationEditorDialog.vue'
import RunHistoryDialog from './components/RunHistoryDialog.vue'
import { automationService } from '../../services/automationService'
import { useAutomationStore } from '../../stores/automation'
import { useProjectStore } from '../../stores/project'
import { formatRelativeTime } from '../../utils/time'
import { automationErrorMessage, RUN_STATUS_LABELS } from '../../types/automation'
import type { Automation } from '../../types/automation'
import { triggerLabel } from './scheduleLabel'

const router = useRouter()
const store = useAutomationStore()
const projectStore = useProjectStore()
const message = useMessage()
const dialog = useDialog()

const editorOpen = ref(false)
const editing = ref<Automation | null>(null)
const historyOpen = ref(false)
const historyOf = ref<Automation | null>(null)
const busyId = ref<string | null>(null)

/**
 * 图标块配色：按 id 稳定散列取一套。
 *
 * 不用名字散列 —— 改名会让卡片换个颜色，看起来像换了一条自动化。
 */
const ICON_TINTS = [
  { wrap: 'bg-blue-50', fg: 'text-blue-500', Icon: FileCode2 },
  { wrap: 'bg-amber-50', fg: 'text-amber-500', Icon: Package },
  { wrap: 'bg-emerald-50', fg: 'text-emerald-500', Icon: Beaker },
  { wrap: 'bg-violet-50', fg: 'text-violet-500', Icon: ListChecks },
  { wrap: 'bg-orange-50', fg: 'text-orange-500', Icon: Wrench },
  { wrap: 'bg-sky-50', fg: 'text-sky-500', Icon: Rocket },
  { wrap: 'bg-rose-50', fg: 'text-rose-500', Icon: Shield },
]

function tintOf(id: string) {
  let hash = 0
  for (let i = 0; i < id.length; i += 1) hash = (hash * 31 + id.charCodeAt(i)) >>> 0
  return ICON_TINTS[hash % ICON_TINTS.length]
}

/**
 * 在飞运行判定 —— **只信父行的 `lastRunStatus`**。
 *
 * 不能拿 `store.runsById` 的历史缓存来判定：那份缓存只在「打开历史弹窗」
 * 或「点过运行」时才更新，run 结算后没人擦除它，缓存里那条 `running` 会让
 * 卡片永远显示「运行中」（实测踩过）。
 *
 * `lastRunStatus` 由 Rust 在 `start_run` / `settle_run` 里原子写回父行，
 * 每次 `automation://changed` 后 store.refresh() 都会重新读到，是权威值。
 *
 * 额外兜底：超过 MAX_RUN_MS 仍是 running 的视为僵死（后端 180 分钟会对账
 * 判死；这里让 UI 先别一直转圈）。
 */
const MAX_RUN_MS = 180 * 60_000

const runningIds = computed(() => {
  const set = new Set<string>()
  const now = Date.now()
  for (const a of store.items) {
    if (a.lastRunStatus !== 'running') continue
    // lastRunAt 是这次运行开始的时间；太久没变化就当它已经死了
    if (a.lastRunAt !== null && now - a.lastRunAt > MAX_RUN_MS) continue
    set.add(a.id)
  }
  return set
})

/** 卡片副标题：优先用户写的提示词首行；没有就用动作说明 */
function descriptionOf(automation: Automation): string {
  const firstLine = automation.prompt.split('\n').find((line) => line.trim())
  if (firstLine) return firstLine.trim()
  return automation.action === 'enqueue_task' ? '把提示词加入任务看板' : '启动一个无头会话'
}

/** 触发方式胶囊文案 */
function scheduleOf(automation: Automation): string {
  if (!automation.enabled) return '已停用'
  return triggerLabel(automation.triggerKind, automation.cron, automation.timezone).replace(
    / · .*$/,
    '',
  )
}

let unlisten: UnlistenFn | null = null

onMounted(async () => {
  try {
    await Promise.all([store.load(), projectStore.loaded ? Promise.resolve() : projectStore.load()])
  } catch (err) {
    message.error(automationErrorMessage(err))
  }
  void store.markSeen()

  unlisten = await automationService.onChanged(() => {
    void store.refresh().catch(() => {})
    if (historyOpen.value && historyOf.value) {
      void store.loadRuns(historyOf.value.id, 30).catch(() => {})
    }
  })
})

onBeforeUnmount(() => {
  unlisten?.()
  unlisten = null
})

function openCreate() {
  editing.value = null
  editorOpen.value = true
}

function openEdit(automation: Automation) {
  editing.value = automation
  editorOpen.value = true
}

function openHistory(automation: Automation) {
  historyOf.value = automation
  historyOpen.value = true
}

async function onSaved() {
  editorOpen.value = false
  await store.refresh().catch(() => {})
}

async function runNow(automation: Automation) {
  if (busyId.value) return
  busyId.value = automation.id
  try {
    await store.runNow(automation.id)
    message.success('已触发一次运行')
    void store.loadRuns(automation.id, 30).catch(() => {})
  } catch (err) {
    message.warning(automationErrorMessage(err))
  } finally {
    busyId.value = null
  }
}

async function cancel(automation: Automation) {
  if (busyId.value) return
  let runId: string | null = null
  try {
    const rows = await store.loadRuns(automation.id, 30)
    runId = rows.find((r) => r.status === 'running')?.id ?? null
  } catch {
    runId = null
  }
  if (!runId) {
    message.info('当前没有在飞的运行')
    return
  }
  busyId.value = automation.id
  try {
    await store.cancelRun(automation.id, runId)
    message.success('已取消')
    void store.loadRuns(automation.id, 30).catch(() => {})
  } catch (err) {
    message.error(automationErrorMessage(err))
  } finally {
    busyId.value = null
  }
}

async function toggle(automation: Automation, enabled: boolean) {
  if (busyId.value) return
  busyId.value = automation.id
  try {
    await store.setEnabled(automation.id, enabled)
  } catch (err) {
    message.error(automationErrorMessage(err))
  } finally {
    busyId.value = null
  }
}

function remove(automation: Automation) {
  dialog.warning({
    title: '删除自动化',
    content: `确定删除「${automation.name}」吗？运行历史会保留。`,
    positiveText: '删除',
    negativeText: '取消',
    onPositiveClick: async () => {
      try {
        await store.remove(automation.id)
        message.success('已删除')
      } catch (err) {
        message.error(automationErrorMessage(err))
      }
    },
  })
}

/** 返回编码工作区（与侧栏的高亮逻辑一致） */
function goCoding(): void {
  void router.push('/coding')
}

/** 历史弹层里取消成功后：刷新列表，让卡片的「运行中」立刻消失 */
async function onHistoryCancelled() {
  try {
    await store.refresh()
  } catch {
    // 刷新失败不打断：下一次 automation://changed 事件会再刷
  }
}

async function refresh() {
  try {
    await store.refresh()
  } catch (err) {
    message.error(automationErrorMessage(err))
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col bg-white">
    <!-- 顶部工具条：与设计图一致，只留刷新（新建在内容区的虚线卡里） -->
    <!--
      单条 header：返回 / 标题 —— 失败徽标 · 刷新 · 窗口三键。

      与 /tasks 保持同一版式（设计图二）：左侧是页面身份，右侧是操作，
      窗口控件与标题同处一层，不再单独占用一条空横条。
    -->
    <header
      data-drag-region
      class="flex h-12 shrink-0 items-center gap-2 border-b border-neutral-200 pr-2 pl-4"
    >
      <button
        class="flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-neutral-500 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
        title="返回编码工作区"
        data-no-drag
        @click="goCoding"
      >
        <ArrowLeft class="h-4 w-4" />
      </button>
      <Zap class="h-4 w-4 shrink-0 text-neutral-400" />
      <h1 class="shrink-0 text-[13px] font-medium text-neutral-800">自动化</h1>

      <span class="flex-1" />

      <span
        v-if="store.unseenFailures > 0"
        class="shrink-0 rounded-full bg-rose-50 px-1.5 py-px text-[10px] font-medium text-rose-600"
      >
        {{ store.unseenFailures }} 次失败
      </span>
      <button
        data-no-drag
        class="flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center rounded-md text-neutral-400 transition-colors hover:bg-neutral-100 hover:text-neutral-700"
        title="刷新"
        @click="refresh"
      >
        <RefreshCw class="h-3.5 w-3.5" />
      </button>
      <WindowControls />
    </header>

    <main class="min-h-0 flex-1 overflow-y-auto">
      <div class="mx-auto w-full max-w-[900px] px-8 pt-12 pb-16">
        <!-- 标题区 -->
        <div class="flex flex-col items-center text-center">
          <div
            class="flex h-12 w-12 items-center justify-center rounded-full bg-neutral-100 text-neutral-500"
          >
            <Zap class="h-5 w-5" />
          </div>
          <h1 class="mt-4 text-[17px] font-semibold text-neutral-900">自动化日常智能体任务</h1>
          <p class="mt-1.5 text-[12px] text-neutral-500">
            安排智能体定时或按需审查代码、更新依赖或处理问题。
          </p>
        </div>

        <p v-if="store.loading" class="py-16 text-center text-[12px] text-neutral-400">
          加载中…
        </p>

        <!-- 卡片网格 -->
        <div v-else class="mt-8 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          <!-- 空白自动化：虚线卡，固定第一格 -->
          <button
            class="flex min-h-[136px] cursor-pointer flex-col items-start rounded-xl border border-dashed border-neutral-300 px-4 py-4 text-left transition-colors hover:border-neutral-400 hover:bg-neutral-50/60"
            @click="openCreate"
          >
            <span
              class="flex h-8 w-8 items-center justify-center rounded-lg border border-neutral-200 bg-white text-neutral-400"
            >
              <Plus class="h-4 w-4" />
            </span>
            <span class="mt-3 text-[13px] font-medium text-neutral-800">空白自动化</span>
            <span class="mt-1 text-[11px] leading-4 text-neutral-500">从零配置一个智能体任务。</span>
          </button>

          <!-- 已保存的自动化 -->
          <div
            v-for="item in store.items"
            :key="item.id"
            class="group relative flex min-h-[136px] cursor-pointer flex-col items-start rounded-xl border border-neutral-200 px-4 py-4 text-left transition-colors hover:border-neutral-300 hover:shadow-[0_1px_3px_rgba(0,0,0,0.04)]"
            :class="item.enabled ? 'bg-white' : 'bg-neutral-50/50'"
            @click="openEdit(item)"
          >
            <!-- 图标块 -->
            <span
              class="flex h-8 w-8 items-center justify-center rounded-lg"
              :class="tintOf(item.id).wrap"
            >
              <component :is="tintOf(item.id).Icon" class="h-4 w-4" :class="tintOf(item.id).fg" />
            </span>

            <!-- 名称 + 失败徽标 -->
            <div class="mt-3 flex w-full min-w-0 items-center gap-1.5">
              <span class="min-w-0 flex-1 truncate text-[13px] font-medium text-neutral-800">
                {{ item.name }}
              </span>
              <span
                v-if="item.unseenFailures > 0"
                class="shrink-0 rounded-full bg-rose-50 px-1.5 py-px text-[10px] font-medium text-rose-600"
                :title="`${item.unseenFailures} 次未查看的失败`"
              >
                {{ item.unseenFailures }}
              </span>
            </div>

            <!-- 说明 -->
            <p class="mt-1.5 line-clamp-2 w-full min-h-8 text-[11px] leading-4 text-neutral-500">
              {{ descriptionOf(item) }}
            </p>

            <!-- 触发方式胶囊 -->
            <div class="mt-auto flex w-full shrink-0 items-center gap-1.5 pt-2.5">
              <span
                class="max-w-full truncate rounded-md px-1.5 py-0.5 text-[10px]"
                :class="
                  item.enabled
                    ? 'bg-neutral-100 text-neutral-600'
                    : 'bg-neutral-100 text-neutral-400'
                "
              >
                {{ scheduleOf(item) }}
              </span>
              <span class="flex-1" />
              <span
                v-if="runningIds.has(item.id)"
                class="flex shrink-0 items-center gap-1 text-[10px] text-blue-600"
              >
                <LoaderCircle class="h-2.5 w-2.5 animate-spin" />
                {{ RUN_STATUS_LABELS.running }}
              </span>
              <span
                v-else-if="item.lastRunAt"
                class="shrink-0 text-[10px] text-neutral-400"
                :title="`最近一次运行：${RUN_STATUS_LABELS[item.lastRunStatus ?? 'succeeded']}`"
              >
                {{ formatRelativeTime(item.lastRunAt) }}
              </span>
            </div>

            <!-- hover 操作条：浮在卡片右上，不挤压布局 -->
            <div
              class="absolute top-2.5 right-2.5 flex items-center gap-0.5 rounded-md bg-white/95 opacity-0 shadow-sm ring-1 ring-neutral-200 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100"
              @click.stop
            >
              <button
                class="flex h-6 w-6 cursor-pointer items-center justify-center rounded text-neutral-500 hover:bg-neutral-100 disabled:opacity-40"
                :title="runningIds.has(item.id) ? '取消运行' : '立即运行'"
                :disabled="busyId === item.id"
                @click="runningIds.has(item.id) ? cancel(item) : runNow(item)"
              >
                <LoaderCircle v-if="busyId === item.id" class="h-3 w-3 animate-spin" />
                <Square v-else-if="runningIds.has(item.id)" class="h-2.5 w-2.5" />
                <Play v-else class="h-3 w-3" />
              </button>
              <button
                class="flex h-6 w-6 cursor-pointer items-center justify-center rounded text-neutral-500 hover:bg-neutral-100"
                title="运行历史"
                @click="openHistory(item)"
              >
                <History class="h-3 w-3" />
              </button>
              <button
                class="flex h-6 w-6 cursor-pointer items-center justify-center rounded text-neutral-500 hover:bg-neutral-100"
                title="启用 / 停用"
                :disabled="busyId === item.id"
                @click="toggle(item, !item.enabled)"
              >
                <Pencil class="h-3 w-3" />
              </button>
              <button
                class="flex h-6 w-6 cursor-pointer items-center justify-center rounded text-neutral-500 hover:bg-rose-50 hover:text-rose-600"
                title="删除"
                @click="remove(item)"
              >
                <Trash2 class="h-3 w-3" />
              </button>
            </div>
          </div>
        </div>

        <!-- 项目提示：没有项目时无法新建 -->
        <p
          v-if="!store.loading && projectStore.projects.length === 0"
          class="mt-6 text-center text-[11px] text-neutral-400"
        >
          还没有打开过任何项目，新建自动化前请先在左侧「打开文件夹」。
        </p>
      </div>
    </main>

    <AutomationEditorDialog
      :open="editorOpen"
      :automation="editing"
      @close="editorOpen = false"
      @saved="onSaved"
    />

    <RunHistoryDialog
      :open="historyOpen"
      :automation="historyOf"
      :load-runs="(id) => store.loadRuns(id, 30)"
      :cancel-run="store.cancelRun"
      @close="historyOpen = false"
      @cancel="onHistoryCancelled"
    />
  </div>
</template>
