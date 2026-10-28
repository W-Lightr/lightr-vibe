// assistant 回复的「内容分区」视图（派生模型，不落盘）。
//
// 为什么需要：现有 `Message` 是「一条消息一类内容」（text / thinking / error），
// 表达不了 codeg 那种「正文 → 工具 → 正文」的混排。分区把一轮消息压平成有序
// 数组，渲染层只认分区，不再自己判断「哪条消息属于哪一段」。
//
// 借鉴 codeg `AdaptedContentPart`，但只保留本仓库真正存在的 4 种。codeg 还有
// delegation / goal / background / plan / compaction 等十几种，本仓库没有那些
// 子系统，搬过来只会是死代码。

/** 回答类分区：这些出现在尾部时构成「最终回答」，永远可见 */
export type AnswerPartType = 'text' | 'turn-error'

/** 过程类分区：这些是「做过什么」，可被折叠 */
export type ProcessPartType = 'reasoning' | 'tool-call'

export interface TextPart {
  type: 'text'
  text: string
}

export interface ReasoningPart {
  type: 'reasoning'
  text: string
  /** 该思考块是否仍在流式输出 */
  streaming: boolean
}

export interface ToolCallPartData {
  type: 'tool-call'
  id: string
  /** agent 给的标题，如「运行了命令」 */
  name: string
  /** ACP 工具类别：read / edit / execute / search / … 缺失时按 other 处理 */
  kind?: string
  /** 命令文本或参数摘要 */
  input: string
  /** 工具输出（可能为空） */
  result?: string
  status: 'running' | 'done' | 'error'
  /** 涉及的文件路径（ACP locations） */
  paths?: string[]
}

export interface TurnErrorPart {
  type: 'turn-error'
  text: string
  /** 与 Message.errorKind 对齐，决定是否显示「去设置」等入口 */
  errorKind?: string
}

/**
 * 一组连续的工具调用。
 *
 * 对齐 codeg 的 `tool-group`：连续的工具不逐条平铺，而是聚合成一个灰色胶囊
 * （「编辑 1 次文件 · 运行 3 个命令」），点开才看到每一条。
 *
 * 只合并**相邻**的工具 —— 中间夹了正文或思考就断开，那说明 agent 换了阶段。
 */
export interface ToolGroupPartData {
  type: 'tool-group'
  items: ToolCallPartData[]
  /** 组内是否有仍在跑的工具（决定胶囊是否 shimmer） */
  streaming: boolean
}


/**
 * AI 提问 + 用户回答（codeg `askQuestionResult` 的简化版）。
 *
 * 为什么单独成一种分区而不是拼成一行文本：问答是**用户参与过**的一次交互，
 * 值得用卡片框出来区别于普通回答（真实数据里它就是一行
 * 「AI 提问：xxx / 我的回答：yyy」混在正文里，很容易被忽略）。
 */
export interface AskResultPart {
  type: 'ask-result'
  /** 问题文本（多问时用 / 连接） */
  question: string
  /** 用户选择的答案（已跳过时为空） */
  answers: string[]
  /**
   * 完整选项列表（老消息没有这段，`options` 为空）。
   * 有它才能在展开时显示所有候选，而不是只显示答案。
   */
  options: { value: string; label: string }[]
  /** 已选中的 value（多选时多个） */
  selected: string[]
  /** single / multi / text */
  kind: string
}

export type ContentPart =
  | TextPart
  | ReasoningPart
  | ToolCallPartData
  | ToolGroupPartData
  | ReplyArtifactsPart
  | AskResultPart
  | TurnErrorPart

/**
 * 一轮回复产生的文件改动汇总（codeg `replyArtifacts` 的简化版）。
 *
 * 为什么单独成一个分区而不是混在工具胶囊里：用户在读完回答后最关心的是
 * 「这轮动了哪些文件」，把它在回答下方单列一张卡比埋进工具列表更可发现。
 *
 * 数据来源限制（与本仓库实际数据核对过）：
 * - edit 类工具的 `input` 是 `{filePath, content}`（整文件写）或
 *   `{filePath, oldString, newString}`（局部替换），JSON 原文完整保留；
 * - **没有 `locations`**（`paths` 为 null），路径只能从 `input.filePath` 取；
 * - 没有「改之前的完整基线」，所以整文件写入**算不出 `+N -M`**，只有
 *   `oldString/newString` 那种局部替换能算。这里用 `lineDelta: null` 表示算不出。
 */
export interface ReplyArtifact {
  /** 文件路径（展示时取 basename） */
  path: string
  /** 该文件在本轮被改动的次数 */
  count: number
  /**
   * 增删行数。`null` = 数据不足（整文件写入没有基线可比）。
   * 有值时按「新增行数 / 删除行数」显示为 `+N -M`。
   */
  lineDelta: { added: number; removed: number } | null
  /**
   * 形似新建：整文件 `content` 写入、没有 `oldString`，且本轮只写过一次。
   *
   * **这是启发式，不是确证**。codeg 靠 unified diff 的 `--- /dev/null` /
   * `new file mode` 精确判断，我们拿不到 diff 原文。
   */
  maybeNew?: boolean
}

/**
 * 一轮的文件改动，分成「新增」与「改动」两组（对齐 codeg 的两个折叠段）。
 *
 * 分类依据见 `ReplyArtifact.maybeNew` 的说明 —— 基于启发式，不是 diff 确证。
 */
export interface ReplyArtifactGroups {
  /** 形似新建的文件 */
  added: ReplyArtifact[]
  /** 改动 / 删除的文件（包含所有无法确证为新增的） */
  changed: ReplyArtifact[]
}

export interface ReplyArtifactsPart {
  type: 'reply-artifacts'
  added: ReplyArtifact[]
  changed: ReplyArtifact[]
}
