# Research: codeg 任务看板 / 卡片 / 编辑器 视觉与结构细节

参考：`H:\AI\Light-vibe\codeg`（只读）。本文件只记录**本次要复刻的部分**及其行号，
供 implement / check 子代理核对。上一轮盘点（数据模型与状态机）已归档在
`archive/2026-10/10-17-task-board/research/codeg-task-board.md`。

---

## 1. 看板列（`src/components/tasks/tasks-page.tsx:817-880`）

### 列头

```tsx
<div className="flex h-6 shrink-0 items-center gap-2 px-0.5">
  {/* 3px 竖色条：echo 列本身，比圆点更像 marker */}
  <span className={cn(
    "h-3.5 w-[3px] shrink-0 rounded-full",
    col === "todo"       && "bg-muted-foreground/50",
    col === "inProgress" && "bg-primary",
    col === "attention"  && "bg-amber-500",
    col === "done"       && "bg-emerald-500",
  )} />
  <h2 className="text-xs font-semibold">{t(COLUMN_LABEL_KEYS[col])}</h2>
  {/* 计数胶囊：attention 且有内容时用琥珀底强调，其余 muted */}
  <span className="rounded-full bg-muted/70 px-1.5 py-0.5 text-[0.625rem] font-medium
                   leading-none text-muted-foreground tabular-nums">{n}</span>
</div>
```

- 列头高度固定 `h-6`（1.5rem），`gap-2`，左右 `px-0.5`
- 标题 `text-xs font-semibold`（12px 半粗）——比我们的 12px medium 更重
- attention 列的计数在有任务时变琥珀（`bg-amber-500/15 text-amber-600`）

### 列容器与空态

```tsx
<div className="flex min-h-0 flex-col gap-2">   {/* 列本身无背景、无边框 */}
  {/* 空态：虚线框，不是灰底 */}
  <div className="flex flex-1 flex-col items-center justify-center gap-1.5
                  rounded-xl border border-dashed border-muted-foreground/30 p-4 text-center">
    <p className="text-xs text-muted-foreground">{t(EMPTY_LABEL_KEYS[col])}</p>
  </div>
  {/* 有内容：ScrollArea + 卡片列表 gap-4 */}
</div>
```

**关键差异**：我们的列是「灰底 + 无虚线框」，codeg 是「白底 + 空列虚线框」。
列与列之间靠 `gap` 分隔（`tasks-page.tsx` 的 grid），没有可见分隔线。

### 列名（`src/i18n/messages/zh-CN.json:5780-5783`）

| key | zh-CN |
|---|---|
| `colTodo` | 待办 |
| `colInProgress` | 进行中 |
| `colAttention` | **等你处理** |
| `colDone` | 已完成 |

空态（`:5762-5765`）：`还没有待办任务` / `暂无进行中的任务` / `暂无等你处理的任务` / `还没有已完成的任务`。

> 我们用「审核」，codeg 用「等你处理」。后者语义更准（包含失败、等输入、待审核）。

---

## 2. 卡片（`src/components/tasks/task-card.tsx:433-576`）

### 外层

```tsx
"group/card flex cursor-pointer flex-col rounded-xl border border-foreground/15
 bg-card ws-msg-card p-3 text-left"
"transition-colors hover:border-primary"     // hover 只变边框色，无位移无阴影
focus-visible:ring-2 focus-visible:ring-ring
archived && "opacity-60"
```

- `rounded-xl`（我们用的 `rounded-lg`）
- `p-3`（12px 内边距，我们用 `px-3 py-2.5`）
- `border-foreground/15`——**不是** `border-neutral-200`，是前景色 15% 透明度，在白底上更淡
- hover 变**主色边框**（蓝），不是变深灰

### 首行：agent 图标 + 标题 + 状态 chip

```tsx
<div className="flex items-start justify-between gap-2">
  <TaskAgentMark task={task} className="mt-[0.125rem]" />   {/* 3.5 尺寸 */}
  <span className="min-w-0 flex-1 break-words text-[0.8125rem] font-medium leading-snug">
    {task.title}
  </span>
  <StatusChip task={task} />
</div>
```

- 标题 13px（`0.8125rem`）medium，`leading-snug`，`break-words`（**不是 line-clamp-2**）
- `TaskAgentMark`（`:337-359`）：有 agent 画 `AgentIcon`，**没有 agent 也画一个灰色 `Bot` 占位**，
  理由是「两个视图都按它对齐标题，占位比每行错位好」
- 状态 chip 在最右，与标题同一行

### 元信息行

```tsx
<div className="mt-1.5 flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-1
                text-[0.6875rem] text-muted-foreground">
  {folderName} <span className="text-muted-foreground/40">/</span> {work_branch}
  <span className="text-muted-foreground/40">·</span>
  {stat}       {/* +12 -3，font-mono */}
  <span className="text-muted-foreground/40">·</span>
  {when}
</div>
```

分隔符是 `muted-foreground/40` 的 `/` 和 `·`。对应到我们：`项目名 · 相对时间`。
（我们没有 work_branch，因为砍了 worktree；也没有 diff stat。）

### 错误条（失败 / 待审核）

```tsx
<div className="mt-2 flex items-center gap-1.5 rounded-lg bg-destructive/10 px-2 py-1.5
                text-[0.6875rem] text-destructive">
  <CircleAlert className="size-3.5 shrink-0" />
  <span className="min-w-0 flex-1 truncate">{task.last_error}</span>
  <button className="shrink-0 font-medium hover:underline">查看</button>
</div>
```

**这是截图里那条红底错误条**：`project folder is not on a branch (detached HEAD?)` + 「查看」。
条件：`task.last_error && (status === 'failed' || status === 'review')`。

`review` 且有 `result_summary` 时再追加两行灰字摘要（`line-clamp-2`）。

### 页脚动作区

```tsx
<div className="mt-3 flex items-center gap-1.5 border-t border-border/60 pt-3">
  {primary && <Button size="xs">            {/* 实心主按钮（默认 variant） */}
    <primary.icon className="size-3" />{primary.label}
  </Button>}
  <div className="flex-1" />
  {secondaries.map(item => <CardIconAction item={item} />)}   {/* 圆形图标按钮 */}
</div>
```

`CardIconAction`（`:568-587`）：
```tsx
<Button size="icon-xs" variant="outline"
  className="rounded-full opacity-0 transition-opacity
             focus-visible:opacity-100 group-focus-within/card:opacity-100
             group-hover/card:opacity-100" />
```

- **主按钮是实心默认样式**（黑底白字，深色主题下），我们做成了描边 + hover 才显示 → 错
- 次按钮是**圆形图标**（`rounded-full variant="outline"`），hover 才显现
- 整行有**上分隔线** `border-t border-border/60 pt-3`，且 `mt-3`
- 主按钮**常驻可见**，只有次按钮 hover 才出现

---

## 3. 新建任务对话框（`src/components/tasks/task-editor-dialog.tsx`）

### 布局（截图 1 对照）

```
┌ 新建任务 ────────────────────────────────── × ┐
│ 标题（无边框 input，约 20px 粗体）              │
│                                                │
│ [图标排] [OpenCode ▾]              恢复继承     │
│ ┌────────────────────────────────────────────┐ │
│ │ 提示词（大 textarea）                       │ │
│ │                                            │ │
│ │ + magpie/Deepseek-V4.1-Flas ▾  Low ▾ build ▾│ │  ← 框内底部栏
│ └────────────────────────────────────────────┘ │
│ 目标                                            │
│ [文件夹 ▾] [HEAD ▾]                            │
│ 任务从该分支拉出工作树，合并时也落回该分支。      │
│ ───────────────────────────────────────────── │
│ [模板]                              取消  保存  │
└────────────────────────────────────────────────┘
```

- 标题：`text-lg` 级无边框输入，占满宽
- 引擎：**下拉 pill**（`AgentSelector`），带当前 agent 图标
- 右侧「恢复继承」：explícit 覆盖后才出现，否则显示继承提示文字
- 提示词框：`max-h-[14rem] min-h-[6rem]`，底部一行是 model / reasoning / mode 下拉
- 「目标」区：小号大写 label（`text-[0.6875rem] font-medium uppercase tracking-wide`）
- 页脚：左「模板」（ghost + 图标），右「取消」「保存」

### 关键代码

- 引擎选择组件：`src/components/chat/agent-selector.tsx`（`variant="pill"`）
- 校验（`:1170-1190` 附近）：标题非空 → 提示词（或附件）非空 → 项目必选 → 无上传中附件
- 保存中 `setSaving(true)`，按钮禁用

> 我们砍掉了 worktree/分支，所以「目标」区只保留**文件夹**一个下拉；
> 「任务从该分支拉出工作树…」这句提示不适用，需要换成本项目的说明（或不显示）。

---

## 4. 我们当前的对应文件

| codeg | 我们 |
|---|---|
| `tasks-page.tsx:817-880`（列头/空态） | `src/views/tasks/components/TaskColumn.vue` |
| `task-card.tsx:433-576`（卡片） | `src/views/tasks/components/TaskCard.vue` |
| `task-card.tsx:337-359`（agent 图标） | 无 |
| `task-editor-dialog.tsx`（编辑器） | `src/views/tasks/components/TaskEditorDialog.vue` |
| `zh-CN.json:5780`（列名） | `src/views/tasks/boardColumns.ts::columnLabel` |
| `StatusChip`（`task-card.tsx:70`） | `src/views/tasks/components/taskChip.ts` + `TaskStatusChip.vue` |

## 5. 本项目的适配差异（必须记牢）

| 点 | codeg | 我们 |
|---|---|---|
| 主色 | shadcn `primary`（跟随主题） | `blue-500` / `blue-600` |
| 卡片边框 | `border-foreground/15` | 用 `border-neutral-200`（项目既有语言，`neutral` 是唯一灰阶） |
| 深色模式 | 有 | 无（light only） |
| work_branch / diff stat | 有 | 无（砍了 worktree） |
| agent 图标 | `AgentIcon` | `src/utils/engineIcons.ts` 已有映射，`SettingsView` 在用 |
| 列名 | 等你处理 | 待办 / 进行中 / **等你处理** / 已完成 |
