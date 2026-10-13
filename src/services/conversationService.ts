// Mock 会话服务：模拟会话创建、消息发送、延迟流式回复与生成状态。
// 与 UI 完全分离，后续可整体替换为真实 API / SSE 流式接口（保持同样的方法签名即可）。
// 注意：所有响应均为本地模拟数据，不代表真实模型调用。

import type { Session, ToolCall } from '../types/session'
import { createId } from '../utils/id'

export interface SendMessageOptions {
  sessionId: string
  content: string
  model: string
  /** 流式增量回调 */
  onDelta: (messageId: string, delta: string) => void
  /** 消息生命周期回调（思考块 / 工具调用 / 助手回复的创建与结束） */
  onEvent: (event: MockEvent) => void
  signal: AbortSignal
}

export type MockEvent =
  | { kind: 'thinking-start'; messageId: string }
  | { kind: 'thinking-delta'; messageId: string; delta: string }
  | { kind: 'thinking-end'; messageId: string }
  | { kind: 'tool-start'; messageId: string; toolCall: ToolCall }
  | { kind: 'tool-end'; messageId: string; toolCallId: string; result: string }
  | { kind: 'reply-start'; messageId: string }
  | { kind: 'reply-end'; messageId: string; usage: { input: number; output: number } }
  | { kind: 'error'; message: string }

const sleep = (ms: number, signal?: AbortSignal) =>
  new Promise<void>((resolve, reject) => {
    const t = setTimeout(resolve, ms)
    signal?.addEventListener('abort', () => {
      clearTimeout(t)
      reject(new DOMException('aborted', 'AbortError'))
    })
  })

/** 根据用户输入生成一段模拟回复（Markdown，覆盖代码块/列表/表格等格式） */
function mockReply(userContent: string): string {
  const topic = userContent.slice(0, 24)
  return [
    `收到你的需求：「${topic}」。以下是本地 Mock 服务生成的模拟回复（非真实模型输出）：`,
    '',
    '### 处理步骤',
    '1. 解析需求并拆解任务；',
    '2. 调用工具读取相关文件；',
    '3. 生成修改方案并写入工作区。',
    '',
    '### 示例代码',
    '```ts',
    'export function greet(name: string): string {',
    '  return `Hello, ${name}!`',
    '}',
    '```',
    '',
    '| 步骤 | 状态 |',
    '| --- | --- |',
    '| 解析 | 完成 |',
    '| 执行 | 完成 |',
    '',
    '> 提示：接入真实后端后，此内容将由 SSE 流式接口提供。',
  ].join('\n')
}

const THINKING_TEXT =
  '用户提出了一个新的需求。先确认当前工作区状态，再决定是否需要读取文件或直接给出方案……'

export const conversationService = {
  /** 创建本地会话记录 */
  createSession(
    title: string,
    model: string,
    opts: { kind: Session['kind']; projectId?: string; storagePath?: string },
  ): Session {
    const now = Date.now()
    return {
      id: createId('ses'),
      title,
      model,
      status: 'idle',
      createdAt: now,
      updatedAt: now,
      gitBranch: opts.kind === 'folder' ? 'main' : '—',
      extensionId: createId('ext'),
      tokenUsage: { input: 0, output: 0, total: 0, contextWindow: 128_000, cachedRead: 0, cachedWrite: 0 },
      messages: [],
      kind: opts.kind,
      projectId: opts.projectId,
      storagePath: opts.storagePath,
    }
  },

  /**
   * 发送一条用户消息并模拟一次完整的 AI 回合：
   * 思考块 → 工具调用 → 流式回复。通过回调推送事件，支持 AbortSignal 停止。
   */
  async send(opts: SendMessageOptions): Promise<void> {
    const { signal } = opts
    try {
      // 1. 思考过程
      const thinkingId = createId('msg')
      opts.onEvent({ kind: 'thinking-start', messageId: thinkingId })
      for (const chunk of THINKING_TEXT.match(/.{1,6}/g) ?? []) {
        await sleep(24, signal)
        opts.onEvent({ kind: 'thinking-delta', messageId: thinkingId, delta: chunk })
      }
      opts.onEvent({ kind: 'thinking-end', messageId: thinkingId })

      // 2. 工具调用
      await sleep(300, signal)
      const toolCall: ToolCall = {
        id: createId('tool'),
        name: 'read_file',
        input: 'src/App.vue',
        status: 'running',
      }
      const toolMsgId = createId('msg')
      opts.onEvent({ kind: 'tool-start', messageId: toolMsgId, toolCall })
      await sleep(600, signal)
      opts.onEvent({
        kind: 'tool-end',
        messageId: toolMsgId,
        toolCallId: toolCall.id,
        result: '<template>\n  <RouterView />\n</template>\n（共 3 行）',
      })

      // 3. 流式回复
      await sleep(250, signal)
      const replyId = createId('msg')
      opts.onEvent({ kind: 'reply-start', messageId: replyId })
      const full = mockReply(opts.content)
      let output = 0
      for (const chunk of full.match(/.{1,4}/gs) ?? []) {
        await sleep(16, signal)
        output += chunk.length
        opts.onDelta(replyId, chunk)
      }
      opts.onEvent({
        kind: 'reply-end',
        messageId: replyId,
        usage: { input: Math.round(opts.content.length * 1.6) + 320, output },
      })
    } catch (err) {
      if ((err as DOMException)?.name === 'AbortError') throw err
      opts.onEvent({ kind: 'error', message: (err as Error)?.message ?? '生成失败' })
      throw err
    }
  },
}
