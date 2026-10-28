# 复刻 codeg 对话界面（核心渲染层）

## Goal

把 codeg 的对话渲染主线搬到 lightr-vibe：以「内容分区（parts）」为单位渲染一条
assistant 回复 —— 中间过程（思考 / 工具调用）折叠成一条可展开的进度行，
**尾部回答始终可见**；底部是一条 turn 统计栏（用时 / token / 复制 / 重新生成 / 分叉）。

参照实现（只读，不要改）：
- `H:\AI\Light-vibe\codeg\src\components\message\completed-turn-content.tsx`
- `H:\AI\Light-vibe\codeg\src\components\message\turn-stats.tsx`
- `H:\AI\Light-vibe\codeg\src\lib\adapters\ai-elements-adapter.ts`
  （`AdaptedContentPart` / `isTurnAnswerPart` / `splitTrailingAnswerParts`）

## Requirements

### R1 内容分区视图（parts）

当前 `Message` 是「一条消息一类内容」（`type: 'text' | 'thinking' | 'error'`），
天然表达不了 codeg 的「正文 → 工具 → 正文」混排。

- 新增**纯函数** `toContentParts(messages: Message[]): ContentPart[]`，把一段消息
  序列压平成有序分区数组。至少覆盖：
  - `text` —— 正文（渲染 markdown）
  - `reasoning` —— 思考
  - `tool-call` —— 工具调用（`kind` / `name` / `input` / `result` / `status`）
  - `turn-error` —— 错误卡
- **不改 `Message` 的持久化形状**（`messages/*.json` 不动）。分区是**派生的只读
  视图**：改持久化会牵动 `useConversation`、会话索引、导入导出，风险远大于收益。

### R2 尾部回答切分（核心算法）

移植 codeg 的 `splitTrailingAnswerParts`：

```
isTurnAnswerPart(part):
  text / turn-error                     → true
  reasoning / tool-call                 → false

splitTrailingAnswerParts(parts):
  从尾部往前跳过所有 answer part，得到最后一个 process part 之后的边界 end
  body     = parts[0..end]     ← 可折叠（进度）
  trailing = parts[end..]      ← 常驻可见（回答）
```

**语义要点**：工具**之前**的文字属于中途说明，属于进度，不参与「回答」判定；
只有最后一次过程操作**之后**的文字才是最终回答。这一点是 codeg 注释里反复强调的，
也是最容易实现错的地方。

### R3 折叠交互

- 一条已完成的回复 = 「进度行（可折叠）」+「尾部回答（常驻）」。
- 当前轮（最新一轮）默认展开；历史轮默认折叠。
- **「发送新消息 → 上方所有轮次收起」**：用 `foldEpoch` 计数器，每次发送 +1；
  按 epoch 判断旧展开状态是否失效（移植 codeg `manualFold` 的思路）。
- 没有可折叠内容时（纯文本回复）不显示进度行。

### R4 turn 统计栏

每条 assistant 回复底部一行：用时 · token · 复制 · 重新生成 · 从此分叉 · 完成时刻
（悬停看完整时间）。

- 重新生成 / 分叉**只在最后一轮可用**（历史轮置灰并说明原因）。
- token 全为 0 时视为「没有数据」，不显示「0 tokens」。

### R5 工具卡片

- 单条工具 = 图标（按 `kind`）+ 标题 + 状态点；展开看 `input` / `result`。
- 连续同类工具合并成一组。
- 命令行用等宽字体 + code 底；结果用 `pre`，限高滚动。

## Acceptance Criteria

- [ ] AC1 「正文 → 工具 → 正文」渲染为：折叠的进度行 + 可见的尾部回答。
- [ ] AC2 工具**之前**的文字被收进进度行，不参与「尾部回答」判定。
- [ ] AC3 纯文本回复不显示进度行。
- [ ] AC4 当前轮默认展开；发送新消息后上一轮自动收起；手动展开/收起在滚动与重渲染后保持。
- [ ] AC5 统计栏显示用时与 token；复制可用；重新生成/分叉只在最后一轮可用。
- [ ] AC6 工具卡片按 `kind` 显示正确图标与标题，展开可看命令与输出。
- [ ] AC7 错误卡在尾部回答区可见（不被折叠吞掉）。
- [ ] AC8 旧会话（无 `kind`、无 usage）正常渲染，不报错、不显示空白统计。
- [ ] AC9 `pnpm build` 与 `cargo test` 通过。

## Out of Scope

- codex 专用可视化卡、委派链路（delegation）、子会话弹窗、MCP 专用卡、图表/mermaid。
- 消息虚拟滚动（codeg 的 `virtualized-message-thread`）。
- 会话导入、消息导航侧栏。
- composer（输入框）改造 —— 本次只动消息渲染侧。

## Notes

- 本次会**取代**现有的 `ActivityGroup.vue`（按轮次收纳）与 `ToolCallPanel.vue`
  （按类别分组），改为 part 驱动的渲染。两者可以在迁移期共存，最终移除。
- 与 prd 里其它 UI 调整的关系：`ActivityGroup` 是前几次迭代的产物，本任务的
  切分规则更精确（区分 body / trailing），迁移完成后以本任务为准。
