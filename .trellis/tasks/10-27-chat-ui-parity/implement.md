# Implement — 复刻 codeg 对话界面（核心渲染层）

按 `design.md` 执行。每步独立可验证；打 ✅ 前必须跑该步的验证命令。

验证命令：

```
pnpm build                              # vue-tsc --noEmit + vite build
cd src-tauri; cargo test                # 确认后端未受影响
```

纯函数逻辑用临时 Python 脚本离线验证（仓库无前端测试框架）。

---

## 阶段 0 · 准备

- [x] 0.1 读 `codeg/src/lib/adapters/ai-elements-adapter.ts` 的
      `AdaptedContentPart` / `isTurnAnswerPart` / `splitTrailingAnswerParts`，
      确认切分语义（尤其「工具之前的文字算进度」）。
- [x] 0.2 读 `codeg/src/components/message/completed-turn-content.tsx` 的
      折叠逻辑（`manualFold` / `foldEpoch` / `roundOpen`）。
- [x] 0.3 读 `codeg/src/components/message/turn-stats.tsx`，看统计栏有哪些字段
      与「缺失时怎么降级」（全 0 usage 视为没有数据）。
- [x] 0.4 确认本仓库现状：`types/session.ts` 的 `Message` / `ToolCall` /
      `TokenUsage`，`useConversation.ts` 里 `usage_update` 的落点。

## 阶段 1 · 分区纯函数（无 UI）

- [x] 1.1 新建 `src/types/contentPart.ts`：`ContentPart` 联合类型（4 种）。
- [x] 1.2 新建 `src/utils/contentParts.ts`：
      - `toContentParts(messages: Message[]): ContentPart[]`
      - `splitTrailingAnswer(parts): { body, trailing }`
      - `ANSWER_PART_TYPES` 常量
- [x] 1.3 关键：`toContentParts` 必须**先判断 `toolCalls`**，再判断 `content`——
      工具消息是 `type:'text'` + 空 content，顺序反了会被当成正文。
- [x] 1.4 离线脚本验证 design §9 的 7 个切分用例。
- [x] **验证**：脚本输出全部符合预期；`pnpm build` 通过（类型无错）。

## 阶段 2 · 叶子组件（可独立预览）

- [x] 2.1 `ToolCallPart.vue`：按 `kind` 选图标；展开看 input / result。
- [x] 2.2 `ContentPartView.vue`：分发渲染 text（MarkdownContent）/ reasoning /
      tool-call / turn-error。
- [x] 2.3 `TurnProgressRow.vue`：折叠箭头 + 图标 + 「活动」+ 工具数 + 用时；
      运行中 spinner + 扫光（复用现有 `.running-label` 样式）。
- [x] **验证**：`pnpm build` 通过。

## 阶段 3 · 轮次组件

- [x] 3.1 `TurnStatsBar.vue`：用时 / token（全 0 不显示）/ 复制 / 重新生成 /
      分叉 / 完成时刻；非最新轮置灰 + tooltip 说明原因。
- [x] 3.2 `AssistantTurn.vue`：组合进度行 + 回答区 + 统计栏；
      `parts` 经 `splitTrailingAnswer` 切分；`trailing` 恒可见。
- [x] 3.3 边界：`trailing` 为空时只渲染进度行（不造空回答区）。
- [x] **验证**：`pnpm build` 通过。

## 阶段 4 · 接入 ConversationView

- [x] 4.1 把消息列表切成「用户消息 → AssistantTurn」的块列表：
      - user 消息 → `MessageItem`
      - 一轮 assistant 消息 → `toContentParts` → `AssistantTurn`
- [x] 4.2 折叠态管理：`openTurns` / `foldEpoch` / `epochOf`（design §6）。
- [x] 4.3 用时计算：本轮首尾消息 `createdAt` 之差（`Message` 没有 `updatedAt`）。
- [x] 4.4 接上「重新生成 / 分叉」到现有 `useConversation` 动作。
- [x] **验证**：`pnpm dev:tauri` 手工走查 AC1–AC7。

## 阶段 5 · 清理旧组件

- [x] 5.1 确认新路径全部跑通后，删除 `ActivityGroup.vue`。
- [x] 5.2 删除 `ToolCallPanel.vue`（其分组规则如需保留，迁进 `ToolCallPart`）。
- [x] 5.3 精简 `MessageItem.vue`：移除 assistant 正文/工具栏分支，只留用户气泡与错误卡。
- [x] **验证**：`pnpm build` + `pnpm dev:tauri` 全量走查。

## 阶段 6 · 收尾

- [x] 6.1 更新 `.trellis/spec/frontend/tauri-bridge.md` 里「对话过程收纳」小节，
      改为 part 驱动的描述（旧的 ActivityGroup 规则作废）。
- [x] 6.2 `pnpm build` + `cargo test` 全绿。
- [ ] 6.3 提交（等用户确认）。

---

## Review Gate

阶段 1 完成后暂停：把 7 个切分用例的结果贴给用户确认（这是整个设计的核心，
错了后面全错）。

阶段 4 完成后暂停：请用户实际看一遍对话渲染效果，再进清理阶段。

## 回滚点

- 阶段 1–3 只新增文件，删掉即可。
- 阶段 4 的切换点只有 `ConversationView.vue` 一处渲染分支，回滚 = 恢复该分支。
- 阶段 5 才删旧组件 —— **在此之前旧组件始终可用**，任何时候都能退回。

---

## 执行偏差记录

实际实现与计划不一致、且已确认合理的地方：

1. **`TurnProgressRow.vue` 已删除**：计划里它是独立组件，实际实现时发现它的内容
   （`工作了 X 秒 ⌄` + 下划线 + 状态位）和 `AssistantTurn` 的标题行完全重合，
   再拆一层只是多一个文件。标题行内联进 `AssistantTurn`。

2. **`ContentPart` 增加第 5 种 `tool-group`**：计划只有 4 种。实机发现连续工具
   逐条平铺会把一屏塞满，补了 codeg 的 `tool-group`（聚合成「编辑 1 次文件 ·
   运行 3 个命令」的胶囊）。由 `groupAdjacentTools` + `groupSummary` 两个纯函数
   实现，只在相邻工具间合并。

3. **`splitTrailingAnswer` 的边界条件修正**：design.md §4 原表把「纯文本」写成
   `body=[]`，实际应为 `body=[text]`。已回改 design.md 并补说明：判断要不要显示
   折叠头应看「body 里有没有过程分区」，而不是「trailing 是否为空」。

4. **`MessageItem.vue` 大幅精简**：assistant 渲染全部移交给 `AssistantTurn` 后，
   它只剩用户气泡一种用途。已移除思考 / 错误 / 正文 / 工具栏四条分支，以及随之
   失效的 7 个依赖（`ToolCallPanel` / `MarkdownContent` / `useConversation` 等）。

5. **未实现 `replyArtifacts` 相关卡片**（文件预览、`+N -M` 行数、新增文件聚合）：
   `ToolCall.input` 已被 `summarizeRawInput()` 摘要化，diff 原文未保留，数据不足。
   与用户确认后按「A 方案：只做视觉层」执行，不编造数据。

