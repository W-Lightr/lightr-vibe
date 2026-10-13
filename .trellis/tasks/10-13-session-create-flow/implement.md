# Implement: 新建会话流程（绑定文件夹 + 延迟入列 + 引擎/文件夹选择弹窗）

上下文顺序：`prd.md` → `design.md` → `.trellis/spec/frontend/state-management.md` +
`hook-guidelines.md` + `component-guidelines.md` + `ui-style.md`。
无 Rust 改动。验证命令：`npx vue-tsc --noEmit`（宿主 `vite build` 可能 OOM）。

## 有序清单

### A. Session store 草稿区（`src/stores/session.ts`）
1. 新增 `const drafts = new Map<string, Session>()`。
2. `get(id)` 改为 `byId.get(id) ?? drafts.get(id) ?? null`。
3. 新增 `createDraft(model, opts)`：建会话（`conversationService.createSession`，
   标题占位统一「新会话」）、设 `messagesLoaded=true`、cwd/storagePath 兜底、`drafts.set`；
   **不 unshift、不 schedulePersistIndex**。
4. 新增 `promoteDraft(id)`：`drafts.delete` + `sessions.value.unshift` + `schedulePersistIndex`。
5. 新增 `discardDraft(id)`：`drafts.delete(id)`（幂等）。
6. 新增 `isDraft(id): boolean`。
7. `deleteSession` 兜底：id 在 drafts 时也 `drafts.delete`。
8. 导出 `createDraft/promoteDraft/discardDraft/isDraft`。
9. 保留旧 `createSession`（仍被 fork/seed 用）；不要破坏其行为。

### B. 发送流（`src/composables/useConversation.ts`）
10. `send(sessionId, content)` 在 `ensureMessages` 之前插入：
    `if (sessionStore.isDraft(sessionId)) sessionStore.promoteDraft(sessionId)`。
11. 确认 `applyAutoTitle`（`:562`）在 promote 之后调用（顺序不变即满足）。

### C. Composer（`src/views/coding/components/MessageComposer.vue`）
12. 新增模块级 `const draftInputs = new Map<string, string>()`（文件内，跨挂载持久）。
13. `draft` ref 初始化/`watch(sessionId)` 恢复：从 `draftInputs.get(sessionId) ?? ''`。
14. `draft` 变化写入 `draftInputs.set(sessionId, draft.value)`。
15. `watch(sessionId)` 切换时（旧 id）：
    - 若 `sessionStore.isDraft(oldId)` 且 `draft.value.trim()===''`（用写入前的旧值判断，
      注意顺序：先读旧 `draft`，再恢复新值）→ `sessionStore.discardDraft(oldId)` +
      `discardConnection`（`closeConnection(oldId)`）+ `draftInputs.delete(oldId)`。
    - 若草稿有文字 → 保留（不删 drafts、不删 map）。
16. 保留原有 reset/`ensureConnected(newId)` 逻辑。

### D. 关标签（`src/views/coding/components/SessionTabs.vue`）
17. `closeTab`（会话标签）：若 `sessionStore.isDraft(sessionId)` →
    `sessionStore.discardDraft(sessionId)` + 清 `draftInputs`（需从 Composer 导出清除函数
    或由 Composer 自行在 unmount/watch 处理；见下）。
    - 简化：Composer 在 `watch(sessionId)` 里对旧草稿统一处理；关标签会卸载 Composer
      （活动标签切换）→ 若关的是活动标签，Composer 会 watch 到 `undefined/newId`。
      但 `discardDraft` 在「关标签」必须**无条件**，而 Composer 只处理「切走且空」。
      → **关标签的丢弃放在 SessionTabs**，Composer 只处理切走。
    - `draftInputs` 清理：把 map 与清除函数导出为模块级（`useConversation.ts` 或单独
      模块），Composer 与 SessionTabs 都能清。**决定**：map 放 `useConversation.ts`
      模块级导出 `setDraftInput/getDraftInput/clearDraftInput`，供双方使用。

### E. NewSessionDialog（`src/views/coding/components/NewSessionDialog.vue`）
18. emits 改 `confirm: [{ engineId: EngineWire; target: NewSessionTarget }]`
    （`NewSessionTarget = { kind:'folder'; projectId:string } | { kind:'tmp' }`，类型放
    `types/session.ts` 或 `types/project.ts`）。
19. 新增文件夹下拉（`projectStore.projects`），含「临时会话（不选文件夹）」选项；
    默认 `recentProjects[0]`，无则 tmp。
20. props 加 `defaultProjectId?: string | null`（父级可预置，仅用于初始选中）。
21. 无文件夹时：只展示 tmp 选项 + 提示「先打开一个文件夹」。

### F. 调用方
22. `SidebarPanel.vue`：
    - `createSessionInProject(projectId)` → **不走弹窗**，直接
      `createDraft`（folder + projectId + rootPath cwd + `engineStore.defaultEngineId`）
      + `openSession`。
    - 通用新建（`:99`）/tmp（`:106`）→ 走弹窗；确认按 `target` 建草稿（folder 用项目
      rootPath 作 cwd；tmp 无 cwd）。
    - `onDialogConfirm` 改用新 emits 形状。
23. `TopBar.vue` / `CodingWorkspace.vue`：`onDialogConfirm` 改用新 target；建草稿（非 createSession）。
24. 三处新建统一替换 `sessionStore.createSession(...)` → `sessionStore.createDraft(...)`，
    随后 `workspace.openSession(id)`。

### G. 验证
25. `npx vue-tsc --noEmit`。
26. 手测（design §8）：文件夹内新建无弹窗；通用新建弹窗选文件夹+引擎；首次发送入列+落盘；
    空草稿切走消失；有字草稿切走保留+切回恢复；草稿关标签消失；已入库会话关标签不变；
    浏览器 dev mock；静态核查索引无未发送草稿 id。
27. trellis-check → 提交。

## 验证命令

```bash
npx vue-tsc --noEmit
rg -n "createDraft|promoteDraft|discardDraft|isDraft" src   # 核查接入点
pnpm dev         # 浏览器 mock 手测
pnpm dev:tauri   # 实机手测
```

## 风险/回滚

- **`get()` 双查**：确认所有消费点通过 `get`（无直接读 `sessions.value` 的会话查找漏网）。
- **Composer 切换顺序**：先读旧 `draft` 判空，再恢复新会话输入，避免误判。
- **draftInputs 归属**：放 `useConversation.ts` 模块级导出，避免循环依赖。
- **关标签 vs 切走**：关标签在 `SessionTabs` 无条件丢弃；切走在 Composer 按空判定。
- **fork/seed 不回归**：保持 `createSession` 原语义。
- 回滚：git 整体 revert 单任务 diff。

## start 前检查
- [ ] `implement.jsonl` / `check.jsonl` 填入真实 spec 条目
- [ ] 用户批准最终规划摘要
