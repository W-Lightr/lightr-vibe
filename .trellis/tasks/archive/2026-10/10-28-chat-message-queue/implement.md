# Implement — 复刻 codeg 会话队列

按 `design.md` 执行。验证命令：

```
pnpm build                    # vue-tsc --noEmit + vite build
cd src-tauri; cargo test      # 确认后端未受影响
```

纯函数用临时 Python 脚本离线验证（仓库无前端测试框架）。

---

## 阶段 0 · 前置确认

- [x] 0.1 读 `useConversation.ts` 的 `send`：确认 `finally` 里
      `workspace.setGenerating(sessionId, false)` 的位置，以及
      `passGate` 的语义（flush 门禁要对齐它）。
- [x] 0.2 读 `MessageComposer.vue` 的 `canSend` / 发送按钮，
      确认生成中禁用的具体位置。
- [x] 0.3 确认 `taskSessionBridge.ts` 是否也会调 `send`（若是，改造不能破坏它）。

## 阶段 1 · 纯函数（无 UI、无状态）

- [x] 1.1 新建 `src/utils/queueFlush.ts`：
      - `shouldQueueDirectSend(fromFlush, queueLength): boolean`
      - `flushRetryDelayMs(now, lastBounceAt, windowMs?): number`
- [x] 1.2 离线脚本验证 design §9 的 1、2 两组用例。
- [x] **验证**：脚本全过；`pnpm build` 通过。

## 阶段 2 · 队列 composable

- [x] 2.1 新建 `src/composables/useMessageQueue.ts`：
      - 模块级 `Map<sessionId, QueuedMessage[]>`
      - `enqueue` / `dequeue` / `requeueFront` / `remove` / `reorder` / `update`
      - `getQueueLength(sessionId)`（同步读）
      - `useQueue(sessionId)` 返回响应式列表
      - `clear(sessionId)`
- [x] 2.2 空串守卫：`enqueue` 内部 `trim()` 后为空直接 return。
- [x] 2.3 离线验证 FIFO 顺序与各操作的边界（design §9 第 3、4 组）。
- [x] **验证**：`pnpm build` 通过。

## 阶段 3 · 发送链路改造

- [x] 3.1 `useConversation.send` 增加 `opts.fromFlush`；生成中且非 flush 时
      **不 return void，而是返回 `'would-queue'`**（调用方据此入队）。
- [x] 3.2 `finally` 末尾调用 `scheduleFlush(sessionId)`。
- [x] 3.3 实现 `flushOnce(sessionId)`：
      门禁（队列非空 / !generating / 不在插入往返）→ `dequeue` → `send({fromFlush:true})`
      → 失败则 `requeueFront` + 记录退避时间戳。
- [x] 3.4 `flushScheduled` 防重入；`setTimeout(0)` 调度。
- [x] **验证**：`pnpm build` 通过。

## 阶段 4 · 输入框接入

- [x] 4.1 `MessageComposer.canSend` 去掉 `!generating`（生成中也可点）。
- [x] 4.2 发送处理：`send` 返回 `'would-queue'` 时 → `enqueue` + 清空草稿 +
      提示「已加入队列」。
- [x] 4.3 队列非空时（即使空闲）也走 `shouldQueueDirectSend` 判定，保 FIFO。
- [x] **验证**：`pnpm build` 通过。

## 阶段 5 · 队列 UI

- [x] 5.1 `MessageQueueList.vue`：非空时渲染，标题「队列 (N)」+ 清空按钮。
- [x] 5.2 `MessageQueueItem.vue`：文本摘要 + 上移 / 行内编辑 / 删除。
      第一条标记「下一个发送」。
- [x] 5.3 挂到 `MessageComposer.vue` 输入框上方。
- [x] **验证**：`pnpm build` 通过。

## 阶段 6 · 收尾

- [x] 6.1 `pnpm build` + `cargo test` 全绿。
- [x] 6.2 更新 `.trellis/spec/frontend/tauri-bridge.md`：新增「会话消息队列」
      小节（门禁四条、FIFO 路由、退避、插入降级）。
- [ ] 6.3 提交（等用户确认）。

---

## Review Gate

阶段 3 完成后暂停：发送链路是最容易出「消息丢失/重复」的地方，请用户
确认设计（尤其是"先发后删、失败回队首"这条）再进 UI。

## 回滚点

- 阶段 1–2 只新增文件，删掉即可。
- 阶段 3–4 改动 `useConversation` / `MessageComposer`，
  回滚 = 恢复 `if (isGenerating) return` 与 `canSend` 的 `!generating`。
- 阶段 5 只新增 UI 文件 + 一处挂载，回滚 = 移除挂载。
