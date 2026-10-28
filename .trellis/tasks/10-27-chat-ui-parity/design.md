# Design — 复刻 codeg 对话界面（核心渲染层）

对应 `prd.md`。技术设计与取舍。执行顺序见 `implement.md`。

---

## 1. 边界与模块划分

```
src/
├── types/contentPart.ts                    # 分区类型定义（新）
├── utils/contentParts.ts                   # 纯函数：toContentParts / splitTrailingAnswerParts（新）
├── views/coding/components/
│   ├── ConversationView.vue                # 改：按「用户消息 / assistant 轮次」切块
│   ├── AssistantTurn.vue                   # 新：一轮 assistant 回复（进度行 + 尾部回答 + 统计栏）
│   ├── ContentPartView.vue                 # 新：单个分区的渲染分发（text/reasoning/tool-call）
│   ├── TurnProgressRow.vue                 # 新：可折叠的进度行（shimmer / 用时）
│   ├── TurnStatsBar.vue                    # 新：底部统计栏（用时/token/复制/重新生成/分叉）
│   ├── ToolCallPart.vue                    # 新：单个/单组工具卡片（替代 ToolCallPanel）
│   ├── MessageItem.vue                     # 保留：用户消息 / 错误卡的渲染（精简）
│   ├── ActivityGroup.vue                   # 删除（被 AssistantTurn 取代）
│   └── ToolCallPanel.vue                   # 删除（被 ToolCallPart 取代）
└── composables/useConversation.ts          # 基本不动（只补 usage 透传，如缺）
```

**分层纪律**：
- `utils/contentParts.ts` 是**纯函数**（无 Vue 依赖），可直接单测。
- 组件只消费分区，不再自己判断「哪条消息属于哪一轮」。
- 不再出现「组件里内联 split 逻辑」的重复实现。

---

## 2. 与 codeg 的差异

| codeg | 本实现 | 原因 |
|---|---|---|
| 消息落库就是 `ContentBlock[]`（part 数组） | `Message[]` 派生为 parts | 不改持久化格式，避免牵动 `useConversation` / 索引 / 导入导出 |
| `AdaptedContentPart` 有 15+ 种类型（delegation / goal / background / plan / compaction…） | 只保留 4 种：text / reasoning / tool-call / turn-error | 本仓库没有那些子系统，保留只会是死代码 |
| React + `memo` + `WeakMap` 折叠缓存 | Vue `ref` + `Map<turnId, boolean>` | Vue 不需要 WeakMap（组件实例生命周期由 v-for key 管理）；按 turnId 键足够稳定 |
| `turn-stats.tsx` 用 `Intl.DateTimeFormat` | 复用 `utils/time.ts` | 仓库已有统一格式化，避免两套 |
| `virtualized-message-thread` | 不虚拟化 | 单会话消息量有限，虚拟化是过度设计 |

**保留的核心不变量**（这是真正值钱的部分）：
1. **尾部回答切分**：`splitTrailingAnswerParts` 的边界定义。
2. **body 可折、trailing 常驻**：回答永不被折叠吞掉。
3. **epoch 级联收起**：发送新消息一次性收起上方全部轮次，不遍历线程。
4. **当前轮默认展开**：正在写的回复不被折叠。

---

## 3. 数据模型（`types/contentPart.ts`）

```ts
export type ContentPart =
  | { type: 'text'; text: string }
  | { type: 'reasoning'; text: string; streaming: boolean }
  | {
      type: 'tool-call'
      id: string
      name: string
      /** ACP kind：read / edit / execute / search / … */
      kind?: string
      input: string
      result?: string
      status: 'running' | 'done' | 'error'
      paths?: string[]
    }
  | { type: 'turn-error'; text: string; errorKind?: string }
```

派生规则（`toContentParts`，输入是**一轮**的 Message 数组）：

| Message | 产出 |
|---|---|
| `role='user'` | 不产生 part（由调用方单独渲染） |
| `type='thinking'` | `{ type:'reasoning', text: content, streaming }` |
| `type='text'` 且 `content` 非空且无 toolCalls | `{ type:'text', text: content }` |
| `type='text'` 且有 toolCalls | 每个 ToolCall → 一个 `tool-call` part（content 若非空则先产 text） |
| `type='error'` | `{ type:'turn-error', text: content, errorKind }` |

> 注意：工具消息在现有实现里是 `type:'text'` + 空 content + `toolCalls`，
> 派生时必须**先看 toolCalls**，否则会被当成正文。

---

## 4. 切分算法（`utils/contentParts.ts`）

```ts
const ANSWER_TYPES = new Set(['text', 'turn-error'])

export function splitTrailingAnswer(parts: ContentPart[]) {
  let end = parts.length
  while (end > 0 && ANSWER_TYPES.has(parts[end - 1].type)) end -= 1
  // 末尾没有过程分区 → 不存在「尾部回答」，全部归 body
  if (end === parts.length) return { body: parts, trailing: [] }
  return { body: parts.slice(0, end), trailing: parts.slice(end) }
}
```

**边界情况**：

| 输入 | body | trailing |
|---|---|---|
| 只有 text | `[text]` | `[]` |
| text → tool | `[text, tool]` | `[]` |
| text → tool → text | `[text, tool]` | `[text]` |
| tool → text | `[tool]` | `[text]` |
| tool → error | `[tool]` | `[error]` |
| 空数组 | `[]` | `[]` |

> **注意 `end === parts.length` 时返回 `body: parts`（不是 `body: []`）。**
> 这与 codeg 的 `splitTrailingAnswerParts` 一致。若返回空 body，调用方会同时收到
> 「body 为空」和「trailing 非空」两个信号，无法区分「没有过程可折」与「过程为空」。

> **这条边界是本任务最容易写错的地方**：`text → tool` 时 trailing 为空，
> 意味着「没有最终回答」。此时 UI 应只显示进度行（折叠），而不是强行造一个空回答区。
>
> **还有一个陷阱**：纯文本回复也是 `trailing` 为空、`body` 承接全部。所以判断
> 「要不要显示进度行」不能看 `trailing` 是否为空，而要**看 `body` 里有没有过程类
> 分区**（`reasoning` / `tool-call`）。

---

## 5. 组件契约

### `AssistantTurn.vue`

```ts
defineProps<{
  parts: ContentPart[]
  /** 是否仍在生成（决定进度行 shimmer、统计栏是否显示用时） */
  streaming: boolean
  /** 是否是最新一轮（决定默认展开、统计栏操作可用） */
  current: boolean
  /** 用时（毫秒），由调用方按本轮首尾消息计算 */
  durationMs: number | null
  usage: TokenUsage | null
  /** 本轮折叠状态（由父级持有，保证 epoch 收起生效） */
  open: boolean
}>()
defineEmits<{ 'update:open': [boolean]; regenerate: []; fork: [] }>()
```

渲染：

```
[进度行]  ▸ 活动 · 12 个工具 · 1 分 20 秒      ← body 非空时才有
[回答区]  MarkdownContent(trailing)             ← trailing 恒可见
[统计栏]  1 分 20 秒 · 1.2k tokens · 复制 · 重新生成 · 分叉
```

### `TurnProgressRow.vue`

- 折叠箭头 + 图标 + 「活动」+ 工具计数 + 用时。
- 运行中：spinner 替代箭头 + 扫光动画（复用现有 `.running-label` 样式）。
- 运行中也**可折叠**（前几轮迭代已经踩过：强制展开会让用户点不动）。

### `ToolCallPart.vue`

- 由 `kind` 决定图标：execute→终端、edit→文件笔、read→文件、search→搜索、other→扳手。
- 显示：图标 + 标题 + 状态点；展开看 input（等宽 code）与 result（pre，限高）。
- **连续同类合并**：由 `toContentParts` 之后的一个轻量分组步骤完成，
  或直接在 `AssistantTurn` 里对相邻 tool-call 做合并（二选一，实施时定）。

### `TurnStatsBar.vue`

| 元素 | 数据来源 | 缺失时 |
|---|---|---|
| 用时 | `durationMs` | 不显示 |
| token | `usage.total` | 不显示（全 0 也算缺失） |
| 复制 | `parts` 里的 text 拼接 | 无 text 时禁用 |
| 重新生成 | emit | 非最新轮置灰 + tooltip 说明 |
| 分叉 | emit | 同上 |
| 完成时刻 | `completedAt` | 不显示 |

---

## 6. 折叠状态管理（`ConversationView.vue`）

```ts
/** 每条 turn 的展开态，键是 turnId */
const openTurns = ref<Record<string, boolean>>({})
/** 每次发送新消息 +1；旧 epoch 的展开态全部失效 */
const foldEpoch = ref(0)
const epochOf = ref<Record<string, number>>({})

function isOpen(turnId: string, isCurrent: boolean): boolean {
  // 当前轮永远展开（除非用户手动收过）
  if (epochOf.value[turnId] !== foldEpoch.value) return isCurrent
  return openTurns.value[turnId] ?? isCurrent
}

function onSend() {
  foldEpoch.value += 1
}
```

**为什么用 epoch 而不是遍历**：一次「发送」要收起上方所有轮次，遍历 O(n) 且会
丢失用户手动的展开记录。epoch 只 +1 一次，旧记录自然失效（codeg 同思路）。

**当前轮例外**：当前轮必须展开（用户在看实时输出），所以 epoch 失效后
`isOpen` 对当前轮返回 `true`。

---

## 7. 与现有组件的关系

| 现有组件 | 处置 |
|---|---|
| `ActivityGroup.vue` | **删除**。它的「按轮次收纳」逻辑被 `AssistantTurn` + split 取代；后者能区分 body/trailing，比它精确 |
| `ToolCallPanel.vue` | **删除**。按类别分组 → 改为 part 级 `ToolCallPart`，或保留分组规则迁移进去 |
| `MessageItem.vue` | **保留但精简**：只负责用户气泡与错误卡；assistant 正文与工具栏移交给 `AssistantTurn` |
| `ConversationView.vue` | 改成「用户消息 → AssistantTurn」的块列表 |

> **迁移策略**：先加新组件与纯函数（不动旧组件），在新路径跑通后再删旧的。
> 不要在同一次提交里既加新又删旧，否则出问题无法二分定位。

---

## 8. 边界情况与失败矩阵

| 情况 | 行为 |
|---|---|
| 一轮只有一条纯文本 | 不显示进度行，只显示回答 + 统计栏 |
| 一轮只有工具、没有回答 | 进度行（默认折叠）；统计栏「复制」禁用 |
| 工具消息但 content 非空 | 先产 text part，再产 tool-call part |
| 旧消息无 `kind` | 图标退回通用扳手；标题用 `name` |
| 旧消息无 usage | 统计栏不显示 token |
| 错误卡在末尾 | 属于 answer part，**不被折叠** |
| 错误卡在中间 | 属于 body，可折叠（符合「中途失败后继续」的语义） |
| 正在流式中途折叠 | 允许（用户可主动收起）；新消息到达时保持用户选择 |
| 会话切换 | `openTurns` 按 turnId 键，不同会话不串 |

---

## 9. 测试计划

**纯函数单测（`cargo test` 不适用，前端无测试框架 → 用 `pnpm build` + 手工）**

> 本仓库没有前端测试运行器（spec 明确写「Tests: None」）。所以：
> - `utils/contentParts.ts` 的边界用**临时脚本离线验证**（同前几轮的做法）；
> - 交互行为靠 `pnpm dev:tauri` 手工走查并记录。

必须验证的切分用例（离线）：

1. text → tool → text：body=[text,tool]，trailing=[text]
2. text → tool：body=[text,tool]，trailing=[]
3. tool → text：body=[tool]，trailing=[text]
4. 纯 text：body=[]，trailing=[text]
5. tool → error：body=[tool]，trailing=[error]
6. 空：body=[]，trailing=[]
7. reasoning → tool → text：body=[reasoning,tool]，trailing=[text]

**构建闸门**：`pnpm build`（`vue-tsc --noEmit`）+ `cargo test`（确认后端未受影响）。

---

## 10. 风险与回滚

| 风险 | 缓解 |
|---|---|
| 切分边界写错 → 回答被吞 | 第 9 节 7 个用例逐条离线验证 |
| 迁移期新旧组件共存导致重复渲染 | 分两步提交：先加新、切换路由渲染、确认后删旧 |
| 折叠态在滚动/重渲染后丢失 | 状态放 `ConversationView`（不在子组件内），按 turnId 键 |
| 性能：每次渲染重算 parts | `computed` 缓存；parts 是纯函数输出，依赖稳定 |
| 旧会话数据结构缺失导致报错 | 所有可选字段都有兜底，AC8 专门验证 |

**回滚**：新组件是新增文件，`ConversationView` 的切换点只有一处（块列表渲染）。
回滚 = 恢复该文件的渲染分支 + 删除新组件，旧组件在第一步提交里**未被删除**。
