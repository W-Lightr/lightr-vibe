// 把「一轮 assistant 消息」压平成内容分区，并按 codeg 的规则切出尾部回答。
//
// 纯函数、无 Vue 依赖 —— 这一步是整套渲染的地基，切错边界会导致回答被折叠吞掉。
// 规则来源：codeg `lib/adapters/ai-elements-adapter.ts` 的
// `isTurnAnswerPart` / `splitTrailingAnswerParts`。

import type {
  AnswerPartType,
  ContentPart,
  ReplyArtifact,
  ReplyArtifactGroups,
  TextPart,
  ToolCallPartData,
} from '../types/contentPart'
import type { Message } from '../types/session'

/**
 * 回答类分区。
 *
 * 判定语义（codeg 注释的关键点）：工具**之前**的文字属于「中途说明」，
 * 只有最后一次过程操作**之后**的文字才是最终回答。所以这里只标类型，
 * 具体哪一段算回答由 `splitTrailingAnswer` 从尾部反推。
 */
export const ANSWER_PART_TYPES: ReadonlySet<ContentPart['type']> = new Set<AnswerPartType>([
  'text',
  'turn-error',
])

/** 该分区是否属于「回答」（尾部可见），否则属于「过程」（可折叠） */
export function isAnswerPart(part: ContentPart): boolean {
  return ANSWER_PART_TYPES.has(part.type)
}

/** 把一条 ToolCall 转成分区 */
function toToolPart(tc: NonNullable<Message['toolCalls']>[number]): ToolCallPartData {
  return {
    type: 'tool-call',
    id: tc.id,
    name: tc.name,
    kind: tc.kind,
    input: tc.input,
    result: tc.result,
    status: tc.status,
    paths: tc.paths,
  }
}

/**
 * 一轮消息 → 有序分区数组。
 *
 * **顺序陷阱**：工具消息是 `type: 'text'` + `content: ''` + 带 `toolCalls`，
 * 所以必须先判 `toolCalls` 再判 `content`，否则一串工具会被当成空正文丢掉。
 *
 * 一条消息同时带正文和工具（少见但可能）时，先产 text 再产 tool-call，
 * 保持「说了什么 → 做了什么」的原始阅读顺序。
 */
export function toContentParts(messages: Message[]): ContentPart[] {
  const parts: ContentPart[] = []

  for (const m of messages) {
    // 用户消息不在这里出现：调用方单独渲染用户气泡
    if (m.role === 'user') continue

    if (m.type === 'thinking') {
      parts.push({
        type: 'reasoning',
        text: m.content,
        streaming: m.status === 'streaming',
      })
      continue
    }

    if (m.type === 'error') {
      parts.push({
        type: 'turn-error',
        text: m.content,
        errorKind: m.errorKind,
      })
      continue
    }

    // 正文（可能为空）优先产出，保证「文字 → 工具」的顺序不倒
    const text = m.content.trim()
    if (text !== '') {
      const p: TextPart = { type: 'text', text: m.content }
      parts.push(p)
    }

    // 工具调用：一个 ToolCall 一个分区，不做合并（合并交给渲染层按需处理）
    for (const tc of m.toolCalls ?? []) {
      parts.push(toToolPart(tc))
    }
  }

  return parts
}

/** 尾部切分结果 */
export interface SplittedParts {
  /** 可折叠的进度部分 */
  body: ContentPart[]
  /** 常驻可见的回答部分 */
  trailing: ContentPart[]
}

/**
 * 从尾部切出「回答」，其余归「进度」。
 *
 * 算法：从数组末尾往前跳过所有回答类分区，得到边界 `end`：
 * - `body = parts[0..end]`（含最后一个过程分区）
 * - `trailing = parts[end..]`
 *
 * 边界行为：
 * - 全是回答类（纯文本 / 空数组）→ `body` 承接全部、`trailing` 为空。
 *   调用方据「body 里有没有过程分区」决定是否显示进度行，**不要**用
 *   `trailing` 是否为空来判断。
 * - `text → tool` → trailing 为空（还没有最终回答），UI 只显示进度行。
 * - `tool → text` → body=[tool]、trailing=[text]。
 */
export function splitTrailingAnswer(parts: ContentPart[]): SplittedParts {
  // 从尾部跳过所有回答类分区，落在最后一个过程分区**之后**
  let end = parts.length
  while (end > 0 && isAnswerPart(parts[end - 1])) end -= 1

  // `end` 走到 0 意味着整段都是回答类，没有任何过程分区 ——
  // 此时不存在「尾部回答」这个概念，全部归 body。
  //
  // 注意：**不能**用 `end === parts.length` 判断「没有过程」。
  // 以纯文本 [text] 为例：循环后 end 从 1 递减到 0，而 parts.length 是 1，
  // 两者不相等 —— 那个条件会漏判，导致 body 为空、把正文全塞进 trailing，
  // 上层据此又会认为「没有进度行」，两个信号互相打架。
  if (end === 0) return { body: parts, trailing: [] }

  return { body: parts.slice(0, end), trailing: parts.slice(end) }
}

/**
 * 把**相邻**的 tool-call 合成 tool-group。
 *
 * 对齐 codeg：连续工具聚成一个胶囊（「编辑 1 次文件 · 运行 3 个命令」），
 * 而不是逐条平铺。中间夹了正文 / 思考就断开 —— 那说明 agent 换了阶段，
 * 合起来会掩盖顺序。
 *
 * 只有单条工具时保持 `tool-call`：胶囊里只写「运行 1 个命令」不如直接
 * 展示那条命令有用。
 */
export function groupAdjacentTools(parts: ContentPart[]): ContentPart[] {
  const out: ContentPart[] = []
  let buffer: ToolCallPartData[] = []

  const flush = () => {
    if (buffer.length === 0) return
    if (buffer.length === 1) {
      out.push(buffer[0])
    } else {
      out.push({
        type: 'tool-group',
        items: buffer,
        streaming: buffer.some((t) => t.status === 'running'),
      })
    }
    buffer = []
  }

  for (const p of parts) {
    if (p.type === 'tool-call') {
      buffer.push(p)
      continue
    }
    flush()
    out.push(p)
  }
  flush()

  return out
}

/**
 * 从一轮的分区里汇总「这轮改了哪些文件」。
 *
 * 为什么需要：回答读完后用户最关心的是「动了什么」，把它在回答下方单列一张卡
 * 比埋进工具列表更可发现。
 *
 * 数据现实（已用真实会话核对）：
 * - edit 类工具的 `input` 是 JSON：`{filePath, content}`（整文件写）或
 *   `{filePath, oldString, newString}`（局部替换）
 * - **没有 `locations`**，路径只能从 `input.filePath` 取；取不到就跳过该条
 * - 整文件写入**没有基线**，算不出增删行数 → `lineDelta: null`
 * - 局部替换有 `oldString/newString`，可以算出增删行数
 *
 * 返回 `null` 表示这轮没有任何文件改动，调用方不渲染卡片。
 */
export function collectReplyArtifacts(parts: ContentPart[]): ReplyArtifactGroups | null {
  /** 按路径聚合，保持首次出现顺序 */
  const byPath = new Map<string, ReplyArtifact>()

  const visit = (p: ContentPart) => {
    if (p.type === 'tool-call') addTool(p)
    else if (p.type === 'tool-group') for (const item of p.items) addTool(item)
  }

  const addTool = (tc: ToolCallPartData) => {
    // 只有编辑类才算「文件改动」；读取/搜索/命令不算
    if (tc.kind !== 'edit' && tc.kind !== 'write' && tc.kind !== 'delete' && tc.kind !== 'move') {
      return
    }
    const info = parseEditInput(tc.input)
    if (!info.path) return

    const existing = byPath.get(info.path)
    if (existing) {
      existing.count += 1
      // 增删行数**只在两次都能算时累加**；任一次算不出（整文件写入没有基线）
      // 就保留已有的那份，而不是清成 null —— 否则「改两次，第一次能算、第二次
      // 整文件覆盖」会把已知的 +N -M 丢掉，用户看到的信息反而更少。
      if (existing.lineDelta && info.lineDelta) {
        existing.lineDelta.added += info.lineDelta.added
        existing.lineDelta.removed += info.lineDelta.removed
      }
      return
    }
    byPath.set(info.path, {
      path: info.path,
      count: 1,
      lineDelta: info.lineDelta,
      // 整文件 `content` 写入且无旧值 → 很可能是新建，但**无法确证**
      maybeNew: info.maybeNew,
    })
  }

  for (const p of parts) visit(p)

  if (byPath.size === 0) return null

  // 分类（对齐 codeg 的「新增文件 / 改动文件」两段）：
  // - 只被写过一次、且是整文件 content 写入、且没有 oldString → 归「新增」
  // - 其余（写过多次、或局部替换）→ 归「改动」
  //
  // 注意：codeg 靠 unified diff 的 `--- /dev/null` / `new file mode` 精确判断，
  // 我们拿不到 diff 原文，只能用上述近似。已在 ReplyArtifactGroups 的注释里写明。
  const added: ReplyArtifact[] = []
  const changed: ReplyArtifact[] = []
  for (const a of byPath.values()) {
    if (a.maybeNew && a.count === 1) added.push(a)
    else changed.push(a)
  }

  return { added, changed }
}

/** 解析 edit 工具的 input，抽路径与（可能的）增删行数 */
function parseEditInput(input: string): {
  path: string | null
  lineDelta: { added: number; removed: number } | null
  /** 形似新建（整文件 content 写入）：只是启发式，不是确证 */
  maybeNew: boolean
} {
  if (!input.trim()) return { path: null, lineDelta: null, maybeNew: false }

  let obj: unknown
  try {
    obj = JSON.parse(input)
  } catch {
    return { path: null, lineDelta: null, maybeNew: false }
  }
  if (typeof obj !== 'object' || obj === null) return { path: null, lineDelta: null, maybeNew: false }

  const rec = obj as Record<string, unknown>
  const path = typeof rec.filePath === 'string' && rec.filePath.trim() ? rec.filePath : null

  // 局部替换：oldString/newString 能直接算增删
  const oldStr = typeof rec.oldString === 'string' ? rec.oldString : null
  const newStr = typeof rec.newString === 'string' ? rec.newString : null
  if (oldStr !== null && newStr !== null) {
    const lineDelta = diffLineDelta(oldStr, newStr)
    return { path, lineDelta, maybeNew: false }
  }

  // 整文件写入：没有基线，算不出增删；但「有 content 无 oldString」形似新建
  const hasContent = typeof rec.content === 'string'
  return { path, lineDelta: null, maybeNew: hasContent }
}

/**
 * 极简行级差异：用 LCS 求最长公共子序列，增删行数 = 两侧长度差。
 *
 * 不做完整的 diff 算法（Myers）—— 这里只需要「+N -M」两个数字，不需要精确的
 * 差异块。用滚动数组把空间压到 O(min(n,m))，避免大文件把内存撑爆。
 */
export function diffLineDelta(
  oldText: string,
  newText: string,
): { added: number; removed: number } {
  const a = oldText.split('\n')
  const b = newText.split('\n')
  const n = a.length
  const m = b.length

  // 单边超过 2000 行不精确计算：数字意义不大，且 O(n*m) 会卡住
  if (n > 2000 || m > 2000) {
    return { added: Math.max(0, m - n), removed: Math.max(0, n - m) }
  }

  let prev = new Array<number>(m + 1).fill(0)
  let cur = new Array<number>(m + 1).fill(0)
  for (let i = 1; i <= n; i += 1) {
    for (let j = 1; j <= m; j += 1) {
      cur[j] = a[i - 1] === b[j - 1] ? prev[j - 1] + 1 : Math.max(prev[j], cur[j - 1])
    }
    const t = prev
    prev = cur
    cur = t
    cur.fill(0)
  }
  const lcs = prev[m]
  return { added: m - lcs, removed: n - lcs }
}

/** 取路径的最后一段（展示用，避免长路径占满卡片） */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean)
  return parts.length > 0 ? parts[parts.length - 1] : path
}

/**
 * 工具组的胶囊文案，按 `kind` 聚合。
 *
 * 对齐 codeg 的 `toolGroup.*` 文案（「编辑 1 次文件 · 运行 3 个命令」）：
 * 同类计数而不是逐条罗列，否则一串工具会把胶囊撑爆。
 *
 * 顺序固定（命令 → 编辑 → 读取 → 搜索 → 其他），这样同一批工具无论顺序如何
 * 生成的文案都一致，滚动时不会跳。
 */
const KIND_PHRASES: { key: string; kinds: string[]; one: string; many: string }[] = [
  { key: 'command', kinds: ['execute'], one: '运行 1 个命令', many: '运行 {n} 个命令' },
  { key: 'edit', kinds: ['edit', 'delete', 'move'], one: '编辑 1 次文件', many: '编辑 {n} 次文件' },
  { key: 'read', kinds: ['read'], one: '读取 1 个文件', many: '读取 {n} 个文件' },
  { key: 'search', kinds: ['search', 'fetch'], one: '搜索 1 次', many: '搜索 {n} 次' },
]

/** 按标题推断类别的兜底（老消息没有 kind） */
function guessKindKey(name: string): string {
  const t = name.toLowerCase()
  if (/(读取|已读|查看|read|cat|open)/.test(t)) return 'read'
  if (/(编辑|修改|写入|write|edit|patch|apply)/.test(t)) return 'edit'
  if (/(搜索|查找|grep|search|find|glob)/.test(t)) return 'search'
  if (/(运行|执行|命令|command|bash|shell|terminal|exec)/.test(t)) return 'command'
  return 'other'
}

/** 一组工具的胶囊文案：「编辑 1 次文件 · 运行 3 个命令」 */
export function groupSummary(items: ToolCallPartData[]): string {
  const counts: Record<string, number> = {}
  for (const tc of items) {
    let key = 'other'
    if (tc.kind) {
      for (const phrase of KIND_PHRASES) {
        if (phrase.kinds.includes(tc.kind)) {
          key = phrase.key
          break
        }
      }
    } else {
      key = guessKindKey(tc.name)
    }
    counts[key] = (counts[key] ?? 0) + 1
  }

  const parts: string[] = []
  for (const phrase of KIND_PHRASES) {
    const n = counts[phrase.key] ?? 0
    if (n <= 0) continue
    parts.push(n === 1 ? phrase.one : phrase.many.replace('{n}', String(n)))
  }
  const other = counts.other ?? 0
  if (other > 0) {
    parts.push(other === 1 ? '其他操作 1 次' : `其他操作 ${other} 次`)
  }

  return parts.length > 0 ? parts.join(' · ') : `${items.length} 个工具`
}

/** 统计分区里的工具数量（进度行摘要用） */
export function countTools(parts: ContentPart[]): number {
  let n = 0
  for (const p of parts) {
    if (p.type === 'tool-call') n += 1
    else if (p.type === 'tool-group') n += p.items.length
  }
  return n
}

/** 收集所有正文（统计栏「复制」用）；多个 text 分区按顺序拼接 */
export function collectText(parts: ContentPart[]): string {
  return parts
    .filter((p): p is TextPart => p.type === 'text')
    .map((p) => p.text)
    .join('\n\n')
    .trim()
}
