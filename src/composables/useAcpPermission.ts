// ACP 权限请求处理：队列 + 执行模式三态策略（PRD 权限 UX 决策）。
// auto    → 立即回首个 allow 选项；
// plan    → 只读白名单工具 allow，其余回首个 reject 选项；
// confirm → 入队渲染 PermissionCard（允许一次 / 本会话总是允许 / 拒绝）。
// 「本会话总是允许」：sessionId→toolName 内存 map，命中即自动回 allow_always
// （无 allow_always 选项时退回首 allow）。切换执行模式即时生效（读 store 现值）。

import { computed, ref } from 'vue'
import type { AcpPermissionEvent, AcpPermissionOption } from '../types/acp'
import { acpService } from '../services/acpService'
import { onPermission, onStatus } from '../services/acpEvents'
import { useSessionStore } from '../stores/session'
import { useWorkspaceStore } from '../stores/workspace'

/** plan 模式只读白名单：工具名/kind 命中即放行（PRD R2） */
const READONLY_TOOL_HINTS = ['read', 'glob', 'grep', 'list', 'search', 'webfetch', 'fetch']
const READONLY_KINDS = ['read', 'search', 'fetch']

/** confirm 模式待审批队列（PermissionCard 消费） */
const queue = ref<AcpPermissionEvent[]>([])

/** 「本会话总是允许」记录：sessionId → toolName 集合 */
const alwaysAllowed = new Map<string, Set<string>>()

function toolNameOf(event: AcpPermissionEvent): string {
  return event.toolCall.title ?? event.toolCall.toolCallId ?? ''
}

function isReadonlyTool(event: AcpPermissionEvent): boolean {
  const kind = event.toolCall.kind
  // kind 是协议语义枚举（§4 ToolKind）：在场即权威——非只读 kind 直接拒绝，
  // 防止「标题含 search/read 字样但实为 edit」的假阳性放行
  if (kind) return READONLY_KINDS.includes(kind)
  // kind 缺失时退回工具名启发式
  const name = toolNameOf(event).toLowerCase()
  return READONLY_TOOL_HINTS.some((hint) => name.includes(hint))
}

/** 按 kinds 优先级选选项（不是按 options 数组顺序）：agent 常把 allow_always
 * 排在 allow_once 前面（协议 §6 示例），「允许一次」按钮必须精确回 allow_once，
 * 否则会意外升级为整会话放行 */
function pickOption(options: AcpPermissionOption[], kinds: string[]): AcpPermissionOption | undefined {
  for (const kind of kinds) {
    const found = options.find((o) => o.kind === kind)
    if (found) return found
  }
  return undefined
}

/** 「允许一次」：优先 allow_once（与 UI 文案一致）；无该选项时退回 allow_always
 * （部分 agent 只提供 always 形态，语义上仍属放行） */
function firstAllow(event: AcpPermissionEvent): AcpPermissionOption | undefined {
  return pickOption(event.options, ['allow_once', 'allow_always'])
}

function firstReject(event: AcpPermissionEvent): AcpPermissionOption | undefined {
  return pickOption(event.options, ['reject_once', 'reject_always'])
}

/** 按策略自动答复；返回 true 表示已处理（无需入队）。
 * 无可用选项时回 false 入队：人工按钮同样选不出选项，用户至少能看见请求。 */
async function autoRespond(event: AcpPermissionEvent): Promise<boolean> {
  const workspace = useWorkspaceStore()
  const mode = workspace.executionMode
  const toolName = toolNameOf(event)
  const allowed = alwaysAllowed.get(event.connId)?.has(toolName)

  let option: AcpPermissionOption | undefined
  if (allowed || mode === 'auto') {
    option = firstAllow(event)
  } else if (mode === 'plan') {
    option = isReadonlyTool(event) ? firstAllow(event) : firstReject(event)
  }
  // confirm 且未命中总是允许 → 入队人工审批
  if (!option) return false
  await acpService.respondPermission(event.connId, event.requestId, option.optionId)
  return true
}

/** 人工答复（PermissionCard 按钮调用）；always=true 记入总是允许 map */
async function respond(event: AcpPermissionEvent, optionId: string | null, always: boolean): Promise<void> {
  queue.value = queue.value.filter((e) => e.requestId !== event.requestId)
  if (always && optionId) {
    const toolName = toolNameOf(event)
    let set = alwaysAllowed.get(event.connId)
    if (!set) {
      set = new Set()
      alwaysAllowed.set(event.connId, set)
    }
    set.add(toolName)
  }
  // 进程已死/请求已被 cancel 收敛时答复会失败：请求已出队，失败静默
  // （错误卡由 send() 的 catch 统一呈现），不向组件抛未处理 rejection
  await acpService.respondPermission(event.connId, event.requestId, optionId).catch(() => undefined)
}

/** 会话级取消：把该会话排队中的请求全部回 cancelled（协议 §5 client 义务） */
function cancelQueued(connId: string): void {
  const doomed = queue.value.filter((e) => e.connId === connId)
  queue.value = queue.value.filter((e) => e.connId !== connId)
  for (const e of doomed) {
    void acpService.respondPermission(e.connId, e.requestId, null).catch(() => undefined)
  }
}

let subscribed = false

/** 订阅权限事件（模块单例，initAcpListeners 后调用一次） */
function ensureSubscribed(): void {
  if (subscribed) return
  subscribed = true
  // 会话进程死亡/断连：Rust 已清 pending_perm，该会话排队中的审批卡直接失效，清出队列
  onStatus((event) => {
    if (event.status === 'dead' || event.status === 'stopped') {
      queue.value = queue.value.filter((e) => e.connId !== event.connId)
    }
  })
  onPermission((event) => {
    void (async () => {
      // 诊断（临时）：确认权限事件是否到达前端
      console.debug('[acp-permission] received', event.connId, event.toolCall?.kind, event.toolCall?.title)
      // 事件携带 connId = 本地会话 id（design §8.1）：直接定位本地会话；
      // 会话已删/不存在 → 直接回 cancelled，避免请求悬挂且 UI 不可达
      const local = useSessionStore().get(event.connId)
      if (!local) {
        await acpService.respondPermission(event.connId, event.requestId, null).catch(() => undefined)
        return
      }
      try {
        if (await autoRespond(event)) return
      } catch {
        // 自动答复失败（进程 dead 等）：仍入队让用户看到，避免请求悬挂
      }
      queue.value.push(event)
    })()
  })
}

/** 当前待审批队列（模板消费；只读视图） */
export function useAcpPermission() {
  ensureSubscribed()
  const workspace = useWorkspaceStore()
  const pending = computed(() => queue.value)

  async function allow(event: AcpPermissionEvent): Promise<void> {
    const option = firstAllow(event)
    await respond(event, option?.optionId ?? null, false)
  }

  async function allowAlways(event: AcpPermissionEvent): Promise<void> {
    const option = pickOption(event.options, ['allow_always', 'allow_once'])
    await respond(event, option?.optionId ?? null, true)
  }

  async function deny(event: AcpPermissionEvent): Promise<void> {
    const option = firstReject(event)
    await respond(event, option?.optionId ?? null, false)
  }

  /** 直接回应某个具体选项（动态按钮用）；仅当该选项是 allow_always 时记入总是允许 */
  async function respondOption(event: AcpPermissionEvent, optionId: string): Promise<void> {
    const opt = event.options.find((o) => o.optionId === optionId)
    await respond(event, optionId, opt?.kind === 'allow_always')
  }

  return {
    pending,
    executionMode: computed(() => workspace.executionMode),
    allow,
    allowAlways,
    deny,
    respondOption,
    cancelQueued,
  }
}
