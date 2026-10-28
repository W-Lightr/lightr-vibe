// ACP elicitation 提问处理：收到 `acp://question`（Questions 型）→ 按 connId 排队，
// 渲染 AskQuestionCard；用户作答 / 跳过 → acp_answer_elicitation + 本地标记已回答 +
// 回填一条可见结果消息（R6，对齐 codeg ask-question-result-card）。
// 本期一次一问（design D3/D4）：收到即渲染，无用户总开关。
// 模块单例（照 useAcpPermission 的 cross-mount 模式）：一次订阅，多个组件共享队列。

import { computed, ref } from 'vue'
import type { AcpQuestion, AcpQuestionEvent } from '../types/acp'
import { acpService } from '../services/acpService'
import { onQuestion, onStatus } from '../services/acpEvents'
import { useSessionStore } from '../stores/session'

/** 待答提问队列（AskQuestionCard 消费）；一次一张（按 connId 过滤当前会话） */
const queue = ref<AcpQuestionEvent[]>([])

let subscribed = false

/**
 * 把问题与答案整理成回填消息文本（R6：用户能看到「问了什么、答了什么」）。
 *
 * 末尾附一段 `\n<<ask-result>>{json}`，携带**题干 + 完整选项列表**。
 *
 * 为什么要塞进 content 而不是新开字段：`Message` 没有通用 payload 列，
 * 改持久化形状会牵动会话索引与导入导出。而渲染成「提问回答：<选项> ⌄
 * 展开看全部选项」这种胶囊形态**必须有选项列表**（否则只能显示答案文本，
 * 点开也没东西可看）。约定一个行首标记，解析失败时退回纯文本，不丢内容。
 */
function buildResultText(event: AcpQuestionEvent, answerLines: string[]): string {
  const prompt = event.questions.map((q) => q.prompt || q.id).join(' / ')
  const answer = answerLines.length ? answerLines.join('、') : '（已跳过）'
  const plain = `AI 提问：${prompt}\n我的回答：${answer}`

  // 只带第一问的选项（本期一次一问，design D3/D4）
  const first = event.questions[0]
  const options = (first?.options ?? []).map((o) => ({ value: o.value, label: o.label }))
  if (options.length === 0) return plain

  const payload = {
    question: first?.prompt || first?.id || '',
    kind: first?.kind ?? 'single',
    options,
    // 已选中的 value（多选时多个）
    selected: answerLines
      .map((label) => first?.options.find((o) => o.label === label)?.value)
      .filter((v): v is string => typeof v === 'string'),
  }
  return `${plain}\n<<ask-result>>${JSON.stringify(payload)}`
}

/** 把用户答案渲染为可读文本（选项 label 优先，回退 value；文本原样） */
function answerLinesFor(event: AcpQuestionEvent, answers: { fieldId: string; values: string[] }[]): string[] {
  const lines: string[] = []
  for (const ans of answers) {
    const q = event.questions.find((x) => x.id === ans.fieldId)
    for (const v of ans.values) {
      const opt = q?.options.find((o) => o.value === v)
      lines.push(opt?.label ?? v)
    }
  }
  return lines
}

/** 订阅提问事件（模块单例，initAcpListeners 后调用一次） */
function ensureSubscribed(): void {
  if (subscribed) return
  subscribed = true
  // 会话进程死亡/断连：Rust 已清 pending_question，该会话排队中的提问卡直接失效
  onStatus((event) => {
    if (event.status === 'dead' || event.status === 'stopped') {
      queue.value = queue.value.filter((e) => e.connId !== event.connId)
    }
  })
  onQuestion((event) => {
    console.debug('[acp-question] received', event.connId, event.questions?.length)
    // 事件携带 connId = 本地会话 id：会话已删/不存在 → 直接回 decline，避免请求悬挂
    const local = useSessionStore().get(event.connId)
    if (!local) {
      void acpService.answerElicitation(event.connId, event.requestId, [], true).catch(() => undefined)
      return
    }
    queue.value.push(event)
  })
}

/** 作答（AskQuestionCard 提交调用）：回传答案 + 出队 + 回填结果消息 */
async function submit(event: AcpQuestionEvent, answers: { fieldId: string; values: string[] }[]): Promise<void> {
  queue.value = queue.value.filter((e) => e.requestId !== event.requestId)
  // 进程已死/请求已收敛时答复失败：请求已出队，失败静默（不抛未处理 rejection）
  await acpService
    .answerElicitation(event.connId, event.requestId, answers, false)
    .catch(() => undefined)
  pushResult(event, answers)
}

/** 跳过 / 拒答：回 decline */
async function skip(event: AcpQuestionEvent): Promise<void> {
  queue.value = queue.value.filter((e) => e.requestId !== event.requestId)
  await acpService.answerElicitation(event.connId, event.requestId, [], true).catch(() => undefined)
  pushResult(event, [])
}

/** 回填一条本地 assistant 消息（R6）：含问题与答案，用户可见 */
function pushResult(event: AcpQuestionEvent, answers: { fieldId: string; values: string[] }[]): void {
  const store = useSessionStore()
  if (!store.get(event.connId)) return
  store.pushMessage(event.connId, {
    role: 'assistant',
    // `ask` 让渲染层把它画成卡片（而不是混进正文）—— 这是用户参与过的交互，
    // 值得在阅读时被一眼认出来。
    type: 'ask',
    content: buildResultText(event, answerLinesFor(event, answers)),
    status: 'done',
  })
}

/** 会话级清理：把该会话排队中的提问全部回 decline（协议 client 义务） */
function cancelQueued(connId: string): void {
  const doomed = queue.value.filter((e) => e.connId === connId)
  queue.value = queue.value.filter((e) => e.connId !== connId)
  for (const e of doomed) {
    void acpService.answerElicitation(e.connId, e.requestId, [], true).catch(() => undefined)
  }
}

/** 当前待答提问队列（模板消费）+ 作答/跳过/会话清理（AskQuestionCard 调用） */
export function useAcpQuestion() {
  ensureSubscribed()
  const pending = computed(() => queue.value)
  return { pending, submit, skip, cancelQueued }
}

export type { AcpQuestion, AcpQuestionEvent }
