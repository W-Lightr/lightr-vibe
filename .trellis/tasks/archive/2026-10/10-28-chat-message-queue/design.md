# Design — 复刻 codeg 会话队列

对应 `prd.md`。技术设计与取舍，执行顺序见 `implement.md`。

---

## 1. 边界与模块划分

```
src/
├── composables/
│   ├── useMessageQueue.ts        # 新：队列纯逻辑（按会话隔离）
│   └── useConversation.ts        # 改：send 不再丢弃；flush 挂在 finally
├── views/coding/components/
│   ├── MessageComposer.vue       # 改：生成中可发送（入队）；渲染队列
│   ├── MessageQueueList.vue      # 新：队列 UI（列表 + 行内操作）
│   └── MessageQueueItem.vue      # 新：单行（文本 / 编辑 / 删除 / 插入）
└── utils/queueFlush.ts           # 新：纯函数 shouldQueueDirectSend / flushRetryDelayMs
```

**分层纪律**：
- 队列的增删改是**纯逻辑**，放 composable，不碰 DOM、不碰 ACP。
- 「什么时候 flush」属于发送链路，放 `useConversation`。
- 纯判定函数（`shouldQueueDirectSend` / 退避）单独成文件，可离线验证。

---

## 2. 与 codeg 的差异

| codeg | 本实现 | 原因 |
|---|---|---|
| `useState` + `useRef` 双写（ref 是权威） | 单一 `reactive`/`ref` | Vue 的 ref 读取是**同步**的，没有 React state 滞后一帧的问题，不需要双份 |
| 队列项是 `PromptDraft`（blocks / modeId / 附件） | `{ id, content: string }` | 本仓库 `send(sessionId, content)` 只收纯文本 |
| 队列存在组件内 hook | 存在 composable（模块级 Map<sessionId, items>） | 队列必须跨组件生命周期存活（切页/切标签回来还在） |
| 队列随会话 tab 生命周期 | 按 sessionId 隔离 | 与 `drafts` 同思路：会话是边界 |
| `onSteerItem` 投递到在飞 turn | **降级**：移到队首并说明 | 见 §6 |
| 拖拽用 `motion/react` 的 Reorder | 原生 HTML5 拖拽或 ↑↓ 按钮 | 不新增依赖 |

---

## 3. 队列模型

```ts
export interface QueuedMessage {
  id: string
  content: string
  createdAt: number
}
```

**存储**：模块级 `Map<sessionId, QueuedMessage[]>`，由 composable 暴露响应式视图。

> 为什么用模块级 Map 而不是放进 Pinia store：队列是**会话的运行期附属状态**，
> 与 `useAcpPermission` / `useAcpQuestion` 的模块单例同构（仓库既有模式）。
> 放 store 会让 store 承担「一个不该持久化的临时态」。

**同步读**：`getQueueLength(sessionId)` 直接读 Map，供发送路径当拍判断。

---

## 4. 发送链路改造（核心）

### 4.1 `send` 不再丢弃

```ts
async function send(sessionId, content, opts?: { fromFlush?: boolean }) {
  if (!content.trim()) return
  // 生成中：不丢弃，交给调用方决定（输入框会走 enqueue）
  if (workspace.isGenerating(sessionId) && !opts?.fromFlush) {
    return 'would-queue' as const   // 显式告诉调用方
  }
  ...
}
```

> **不在这里直接入队**：`send` 是被多处调用的底层函数（含 taskSessionBridge），
> 在底层偷偷入队会让那些调用方拿到"已发送"的假象。改为返回明确结果，由
> 输入框决定。

### 4.2 flush 触发点

`send` 的 `finally` 里已经有 `workspace.setGenerating(sessionId, false)`。flush 挂在
它**之后**：

```ts
finally {
  ...
  workspace.setGenerating(sessionId, false)
  scheduleFlush(sessionId)   // ← 新增
}
```

`scheduleFlush` 用 `setTimeout(0)`：让 `finally` 里的状态写入先落定，避免在同一个
同步块里递归调用 `send`。

### 4.3 flush 的四道门禁（对齐 codeg）

```
canFlush(sessionId):
  1. 队列非空
  2. !isGenerating(sessionId)          ← 一次只发一条
  3. 会话可发送（复用 send 内部的 passGate 语义，见下）
  4. 不在「插入在飞 turn」的往返中
```

> **第 3 条为什么要复用**：codeg 注释的核心教训 —— 任何比 `send` 自身更宽松的
> 门禁，都会「把消息从队列取出来，然后看着 send 静默丢弃」。
> 本实现的做法：**先 dequeue 再 send，若 send 返回 would-queue/失败，则 requeueFront**。
> 这样即使门禁判断有偏差，消息也不会丢。

### 4.4 退避

```ts
export function flushRetryDelayMs(now: number, lastBounceAt: number, windowMs = 1000): number
```

被拒（`would-queue` 或异常）后记录时间戳，下次 flush 等剩余窗口。

---

## 5. FIFO 路由规则

```ts
export function shouldQueueDirectSend(fromFlush: boolean, queueLength: number): boolean {
  if (fromFlush) return false          // 自动 flush 直接发
  return queueLength > 0               // 用户直发：队列非空则尾部入队
}
```

| 场景 | 结果 |
|---|---|
| 空闲 + 队列空 + 用户直发 | 直发 |
| 空闲 + 队列非空 + 用户直发 | **尾部入队** |
| 生成中 + 用户直发 | 尾部入队（无队列时也是第一条） |
| 自动 flush | 直发（失败回队首） |

---

## 6. 「插入在飞 turn」的降级（重要）

codeg 的 `onSteerItem` 把消息投递给**正在跑的 turn**，走的是它的 live-feedback
通道。本仓库没有这个通道：`acp_prompt` 是"发一条、等一个 stopReason"的阻塞调用，
`acp.rs` 里也没有并发 prompt 的支持。

**降级方案**（不假装能做到）：

- 按钮仍保留，语义改为「**优先发送**」：把该项移到队首。
- UI 文案如实写「移到队首，本轮结束后优先发送」，不写"插入当前轮"。
- 该项在队列中高亮标出「下一个发送」。

> 若将来 `acp.rs` 支持 turn 中追加 input，再把这里换成真正的插入。**现在不做
> 假的实现** —— 那会让用户以为消息已经进了这一轮。

---

## 7. 队列 UI

```
┌─────────────────────────────────────────┐
│ 队列 (3)                        [清空] │
├─────────────────────────────────────────┤
│ ⠿ 帮我把测试也补上            [↑][✎][✕] │
│ ⠿ 再看看有没有遗漏的边界       [↑][✎][✕] │
│ ⠿ 最后跑一遍构建               [↑][✎][✕] │
└─────────────────────────────────────────┘
```

- 位置：输入框**上方**（对齐 codeg），队列为空时整块不渲染。
- 排序：`↑` 上移按钮（不引拖拽库；原生 HTML5 拖拽在 Tauri WebView 里行为不
  一致，成本高于收益）。
- 编辑：点击 ✎ 把内容回填到输入框并**从队列暂离**，取消时放回原位。
  > 简化：直接行内编辑（`textarea` 替换文本）比回填输入框更少状态、更少 bug。
- 删除：`✕`。

---

## 8. 边界情况与失败矩阵

| 情况 | 行为 |
|---|---|
| 生成中连发 3 条 | 依次入队；本轮结束后逐条发出 |
| 发送过程中用户又发一条 | 入队尾（此时 `isGenerating` 为真） |
| flush 取出的那条发送失败 | 回**队首**，退避后重试 |
| 用户直发被判定为入队 | 入**队尾**；若随后 flush 失败，不影响它 |
| 会话被删除 | 队列随之清理（`Map.delete`） |
| 会话切换 | 队列按 sessionId 隔离，互不影响 |
| 停止生成（用户点停） | `finally` 照常 `setGenerating(false)` → 触发 flush（符合预期：用户只是停了当前轮） |
| 队列项内容为空 | 入队前 `trim()` 守卫，空串不入队 |
| 同一会话重复触发 flush | `flushScheduled` 标记防重入 |
| 页面卸载 | 模块级 Map 存活；组件卸载只影响 UI，不影响队列 |

---

## 9. 测试计划

仓库无前端测试框架，纯函数用离线脚本验证：

1. `shouldQueueDirectSend`：四种组合（见 §5 表）
2. `flushRetryDelayMs`：无退避 / 窗口已过 / 窗口内（返回剩余）
3. 队列操作：enqueue / dequeue / requeueFront / remove / reorder / update
4. FIFO 顺序：连续入队 3 条，依次 dequeue 得到入队顺序

**构建闸门**：`pnpm build` + `cargo test`。

---

## 10. 风险与回滚

| 风险 | 缓解 |
|---|---|
| 门禁比 send 宽松 → 消息被取走又丢弃 | **先发后删**：发送成功才移除；失败回队首（§4.3） |
| flush 递归触发（自己触发的 send 又触发 flush） | `flushScheduled` + `setTimeout(0)` + `isGenerating` 门禁 |
| 队列跨会话串味 | Map 按 sessionId 键；读时按当前会话 |
| 用户不知道消息去哪了 | 队列 UI 一直可见（非空时），且入队时输入框清空有明确反馈 |
| 插入在飞 turn 是假的 | 已降级并如实标注（§6） |

**回滚**：新增文件删除 + `send` 恢复 `if (isGenerating) return` +
`canSend` 恢复 `!generating` + 移除 `scheduleFlush` 调用。改动面集中在
3 个文件，回滚清晰。
