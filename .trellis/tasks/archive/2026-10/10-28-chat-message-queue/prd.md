# 一键复刻 codeg 会话队列（连续发送排队、适机发送）

## Goal

对齐 codeg 的会话队列：**agent 正在生成时，用户继续发送的消息不丢弃、不打断当前
turn，而是进入队列；当前 turn 结束后自动把队首发出去**。队列可查看、可排序、
可编辑、可删除，也能主动"插进正在跑的这一轮"。

参照实现（只读）：
- `codeg/src/hooks/use-message-queue.ts` —— 队列数据结构与增删改
- `codeg/src/components/conversations/conversation-detail-panel.tsx` 第 860–945 行
  —— 自动 flush 引擎（四个门禁 + 退避）
- `codeg/src/lib/queue-flush.ts` —— `shouldQueueDirectSend` / `flushRetryDelayMs`
- `codeg/src/components/chat/message-queue-display.tsx` —— 队列 UI

## 复刻前的现状差异（决定了要改哪里）

| 项 | codeg | 本仓库现状 |
|---|---|---|
| 生成中发送 | 入队，turn 结束后自动发 | **直接丢弃**（`useConversation.send` 第 829 行 `if (isGenerating) return`） |
| 发送按钮 | 生成中仍可点（入队） | `canSend` 含 `!generating`，**按钮禁用** |
| 队列存储 | `useMessageQueue` hook（hook 内 ref + state） | 无 |
| 队列 UI | 输入框上方可拖拽列表 | 无 |

## Requirements

### R1 队列模型

一条队列项至少包含：`id` / `content`（发送文本）/ `createdAt`。

操作：
- `enqueue(content)` —— 追加到队尾
- `dequeue()` —— 取队首并移除，返回该项
- `requeueFront(content)` —— 放回**队首**（自动发送被拒后的重试，保 FIFO）
- `remove(id)` / `reorder(orderedIds)` / `update(id, content)`

> **同步读**：必须能同步取得「当前队列长度」，供发送路径当拍判断走直发还是入队。
> codeg 用 `queueRef` 保存权威副本的原因是 React state 滞后一帧；Vue 的 ref 是
> 同步的，这一点本仓库天然满足 —— 但**不能**在 computed 里读后跨 `await` 使用。

### R2 生成中发送 → 入队

- `useConversation.send` 在 `isGenerating` 时**不再直接 return**，改为把内容交给
  队列（由调用方决定：输入框调用时入队）。
- 输入框发送按钮在生成中**保持可点**，点击后清空草稿 + 入队（对齐 codeg）。

### R3 自动 flush（核心）

agent 空闲时自动把队首发出去。**四个门禁，缺一不可**：

1. **会话可发送**（连接就绪 / 非 dead）—— 必须复用 `send` 自己的就绪判断，
   不能另写一套更宽松的，否则会把消息从队列取走再被 `send` 静默丢弃。
2. **上一次发送已落定**（`!isGenerating`）—— 保证一次只发一条。
3. **没有"插入在飞 turn"的操作正在进行**（R6）—— 否则同一句会被读两遍。
4. **队列非空**。

触发时机：用 `setTimeout(0)` 而不是同步/微任务 —— 让当前 turn 的收尾状态先落定。
被拒后的重试要按退避窗口等待。

### R4 直接发送的路由规则（保 FIFO）

`shouldQueueDirectSend(fromFlush, queueLength)`：

| 来源 | 队列状态 | 行为 |
|---|---|---|
| 自动 flush | — | 直接发；被拒则**队首**回插 |
| 用户直发 | 队列为空 | 直接发；被拒则**队尾**入队 |
| 用户直发 | **队列非空** | **尾部入队**（不插队） |

> 第三条容易漏：用户空闲时手动发一条，如果队列里还压着东西，直发会破坏 FIFO。

### R5 队列 UI

输入框上方展示队列（对齐 codeg）：
- 每行显示文本摘要
- 可拖拽排序、可编辑、可删除
- 生成中时每行提供「插入当前轮」按钮

### R6 插入在飞 turn

生成中把某条队列项**直接投递给正在跑的这一轮**，而非等它结束。

- 往返期间该行**仍留在队列里**（只有后端确认投递后才移出）。
- 此操作进行时，R3 的自动 flush 必须让路（否则同一条被发两次）。

> **本仓库待确认**：opencode 的 `acp_prompt` 是否允许在一个 turn 未结束时再发一次。
> 若协议层面不支持并发 prompt，R6 退化为「把该项移到队首并等本轮结束」，并在 UI
> 上如实说明（不假装已插入）。

### R7 退避

自动 flush 因"上一条还在跑"被拒后，按固定窗口退避，避免每次往返都发一次必然
失败的请求。

## Acceptance Criteria

- [ ] AC1 生成中发送：消息进入队列，当前 turn **不被打断**。
- [ ] AC2 当前 turn 结束后，队首消息**自动发出**，队列相应减一。
- [ ] AC3 连续发 3 条：按 FIFO 顺序依次发出，不插队、不丢失、不重复。
- [ ] AC4 队列非空时用户手动直发 → 进入**队尾**（不插队）。
- [ ] AC5 发送被拒（生成中）→ 自动 flush 的那条回到**队首**，用户直发的那条回**队尾**。
- [ ] AC6 被拒后按退避窗口重试，不会每次往返都打一次空请求。
- [ ] AC7 队列可编辑 / 删除 / 拖拽排序，改动即时反映到后续发送顺序。
- [ ] AC8 生成中队列已空 → 输入框恢复正常直发；生成中队列非空 → 点击只入队。
- [ ] AC9 切换会话时，队列**按会话隔离**，不串会话。
- [ ] AC10 `pnpm build` 与 `cargo test` 通过。

## Out of Scope

- 附件 / 图片入队（本仓库的 `send` 只收纯文本）。
- 队列持久化到磁盘（codeg 也是内存态，刷新即丢）。
- 模式（mode）随队列项一起保存 —— 本仓库 `send` 没有 mode 参数。

## Notes

- 本仓库 `send(sessionId, content)` 只接受纯文本，比 codeg 的 `PromptDraft`
  简单得多；队列项因此可以只存字符串。
- 与 `chat-ui-parity` 的关系：那个任务改的是「渲染」，本任务改的是「发送链路」。
  两者改动面不重叠，但本任务依赖它已经落地的 `useConversation` 结构。
