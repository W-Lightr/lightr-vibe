# Design: 任务看板对齐 codeg UI

需求 `prd.md`，codeg 行号与逐项差异见 `research/codeg-ui-details.md`。
视觉规范：`.trellis/spec/frontend/ui-style.md`。

## 1. 总原则

**照抄结构，换算色板。** codeg 用 shadcn token（`primary` / `border` / `muted-foreground` /
`destructive` / `card`），本项目用显式 Tailwind 色阶。换算表：

| codeg token | 本项目 |
|---|---|
| `primary` | `blue-500`（填充）/ `blue-600`（hover） |
| `border` / `border-border` | `neutral-200` |
| `border-foreground/15` | `neutral-200`（白底上等价效果） |
| `muted` / `muted/70` | `neutral-100` / `neutral-100` |
| `muted-foreground` | `neutral-500` |
| `muted-foreground/40`（分隔符） | `neutral-300` |
| `destructive` | `red-600`，底 `red-50` |
| `amber-500` / `amber-600` | 同名（已是 Tailwind 语义色） |
| `emerald-500` | 项目既有成功色为 `green-600`；**竖条用 `green-500`** |
| `card`（卡片底色） | `white` |
| `rounded-xl`（卡片/空列） | 同名 |

理由：`ui-style.md` 规定 neutral 是唯一灰阶、blue 是唯一非灰 UI 色、红/绿/琥珀仅表语义。
直接照抄 `foreground/15` 这类透明前景色会让深浅对比偏离项目既有语言。

## 2. 列（子任务 1）

### 2.1 列头

```
▎ 待办   1
```

```html
<header class="flex h-6 shrink-0 items-center gap-2 px-0.5">
  <span class="h-3.5 w-[3px] shrink-0 rounded-full" :class="accentClass" />
  <h2 class="text-xs font-semibold text-neutral-700">{{ label }}</h2>
  <span class="rounded-full px-1.5 py-0.5 text-[10px] font-medium leading-none tabular-nums"
        :class="countClass">{{ n }}</span>
</header>
```

- `accentClass`：`todo → bg-neutral-400`、`inProgress → bg-blue-500`、
  `attention → bg-amber-500`、`done → bg-green-500`
- `countClass`：`attention` 且 `n > 0` → `bg-amber-50 text-amber-600`；其余
  `bg-neutral-100 text-neutral-500`

列头高度 `h-6`，与 codeg `tasks-page.tsx:821` 一致。

### 2.2 列容器与空态

**列本身不再有背景与边框。** 空列用虚线框：

```html
<!-- 列 -->
<div class="flex min-h-0 flex-1 flex-col gap-2" :data-column-id>
  <!-- 列头（上） -->
  <!-- 空态 -->
  <div class="flex flex-1 flex-col items-center justify-center gap-1.5
              rounded-xl border border-dashed border-neutral-300 p-4 text-center">
    <p class="text-xs text-neutral-400">{{ hint }}</p>
    <!-- 待办列额外给「新建任务」 -->
  </div>
  <!-- 有内容 -->
  <div ref="columnEl" class="min-h-0 flex-1 overflow-y-auto">
    <!-- 卡片，间距 gap-3 -->
  </div>
</div>
```

拖拽高亮：合法落列时 `ring-2 ring-blue-400 ring-offset-0`（整列）+ `bg-blue-50/40`。
非法落列**不高亮**（沿用现有 `hoverColumn` 语义）。

### 2.3 列命名

`boardColumns.ts::columnLabel`：

| id | 新文案 | 旧文案 |
|---|---|---|
| todo | 待办 | 待办 |
| inProgress | 进行中 | 进行中 |
| attention | **等你处理** | 审核 |
| done | 已完成 | 已完成 |

`emptyColumnHint` 同步对齐 codeg（`:5762-5765`）：

| id | 文案 |
|---|---|
| todo | 还没有待办任务 |
| inProgress | 暂无进行中的任务 |
| attention | 暂无等你处理的任务 |
| done | 还没有已完成的任务 |

> 「等你处理」比「审核」更准：该列同时装着 `failed`、`awaiting_input`、`review`、`merging`。

### 2.4 看板容器

`TasksView.vue` 的 `<main>`：`gap-4 p-4`（原来是 `gap-3 p-3`），列之间无分隔线。
工具栏保持现状（项目下拉、两个开关、任务计数、新建按钮），只把 `height` 与列头对齐。

## 3. 卡片（子任务 2）

### 3.1 结构

```
[◈] 标题................................ [待办]
项目名 · 2 分钟前
┌──────────────────────────────────────────┐
│ ⚠ project folder is not on a branch... 查看│   ← 仅 failed / review 且有 lastError
└──────────────────────────────────────────┘
────────────────────────────────────────────
[▶ 开始]                          (○)(○)        ← 次按钮 hover 才显现
```

### 3.2 外层

```html
<article class="group flex cursor-grab flex-col rounded-xl border border-neutral-200
                bg-white p-3 text-left transition-colors
                hover:border-blue-400 active:cursor-grabbing">
```

- **去掉**上一轮加的 `hover:shadow-[...]`——codeg 明确「no lift, no shadow」
- hover 变**蓝色边框**（codeg 是 `hover:border-primary`）

### 3.3 首行

```html
<div class="flex items-start justify-between gap-2">
  <EngineMark :engine-id="task.engineId" class="mt-[2px]" />   <!-- 14px -->
  <span class="min-w-0 flex-1 break-words text-[13px] font-medium leading-snug text-neutral-800">
    {{ task.title }}
  </span>
  <TaskStatusChip :task="task" />
</div>
```

- 标题**不再 line-clamp-2**，改 `break-words`（codeg 用自然换行）
- `EngineMark` = 新的小组件：`engineIcon(task.engineId)` 有图标就 `v-html` 渲染，
  **没有就画灰色 `Bot` 占位**（保证两个视图标题左对齐，codeg `task-card.tsx:337-359` 同样理由）

### 3.4 元信息行

```html
<div class="mt-1.5 flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-1
            text-[11px] text-neutral-500">
  <span v-if="projectMissing" class="rounded bg-neutral-100 px-1 text-[10px] text-neutral-400">项目已移除</span>
  <span v-else-if="projectName" class="max-w-40 truncate">{{ projectName }}</span>
  <span v-if="projectName || projectMissing" class="text-neutral-300">·</span>
  <span class="shrink-0 tabular-nums">{{ formatRelativeTime(task.updatedAt) }}</span>
</div>
```

### 3.5 错误条

```html
<div v-if="showError" class="mt-2 flex items-center gap-1.5 rounded-lg bg-red-50 px-2 py-1.5
                              text-[11px] text-red-600">
  <CircleAlert class="h-3.5 w-3.5 shrink-0" />
  <span class="min-w-0 flex-1 truncate">{{ task.lastError }}</span>
  <button class="shrink-0 font-medium hover:underline" @click.stop="emit('open')">查看</button>
</div>
```

`showError = task.lastError && (status === 'failed' || status === 'review')`（与 codeg 同条件）。

`review` 且有 `resultSummary` 时，错误条下方再补两行摘要：
`<p class="mt-1.5 line-clamp-2 text-[11px] leading-snug text-neutral-500">`。

### 3.6 页脚

```html
<div v-if="primaryLabel || secondaryLabels.length" class="mt-3 flex items-center gap-1.5
     border-t border-neutral-100 pt-3">
  <!-- 主按钮：实心，常驻 -->
  <button class="flex h-6 items-center gap-1 rounded-md bg-neutral-900 px-2 text-[11px]
                 font-medium text-white transition-colors hover:bg-neutral-800
                 disabled:opacity-40">
    <component :is="primaryIcon" class="h-3 w-3" />{{ primaryLabel }}
  </button>
  <span class="flex-1" />
  <!-- 次按钮：圆形图标，hover 显现 -->
  <button v-for="s in secondaryActions" :key="s.key"
          class="flex h-6 w-6 items-center justify-center rounded-full border border-neutral-200
                 bg-white text-neutral-500 opacity-0 transition-opacity
                 group-hover:opacity-100 group-focus-within:opacity-100"
          :title="s.label">
    <component :is="s.icon" class="h-3 w-3" />
  </button>
</div>
```

- **主按钮实心 `bg-neutral-900 text-white`**（codeg 的默认按钮在浅色主题下就是黑底）
  ——与项目里 `新建会话` 之外的黑按钮语言一致；审核列「通过」仍可用 `bg-blue-500` 表主行动，
  由 `taskChip.ts` 新增的 `primaryTone(status)` 单一决定，避免卡片/详情两处分叉
- 次按钮**圆形图标**（`rounded-full h-6 w-6`），hover 显现，**占位不塌**（codeg 同理：
  「the row keeps its height, so nothing reflows」）

### 3.7 交互不回归

- 卡片本体 `pointerdown` 仍交给 `useTaskDrag`；按钮区 `@pointerdown.stop`
- 点击卡片（非按钮）打开详情；拖拽后 300ms 抑制 click（现有 `shouldSuppressClick`）
- 错误条里的「查看」也 `@click.stop` 后 emit `open`

## 4. 编辑器（子任务 3）

### 4.1 布局

```
新建任务                                         ×
──────────────────────────────────────────────────
标题（无边框，text-lg font-medium）                  ← 无 label、无边框、无背景
[◈ OpenCode ▾]                          恢复继承     ← 引擎下拉 + 继承提示
┌────────────────────────────────────────────────┐
│ 提示词（textarea，min-h-[6rem] max-h-[14rem]）    │
└────────────────────────────────────────────────┘
目标
[📁 项目名 ▾]
──────────────────────────────────────────────────
                              取消   保存
```

### 4.2 引擎下拉

现状已有 `engineDropOpen` + `engineIcon` + `engineStore.agents` 的手写下拉（`TaskEditorDialog.vue:219-250`）。
本轮**保留该实现**，只做视觉对齐：

- trigger 变成 `pill`：`rounded-full border border-neutral-200 bg-white px-2.5 h-7`
- 左侧 16px 引擎图标（`v-html="engineIcon(engineId)"`），右侧 `ChevronDown`
- 展开面板：`rounded-lg border border-neutral-200 bg-white py-1`，每项
  `flex h-8 items-center gap-2 px-2 text-[13px] hover:bg-neutral-100`，选中项打勾
- **不用 naive-ui `NSelect`**：项目既有下拉都是手写（`ui-style.md`「naive-ui 只用行为重的控件」），
  且需要渲染 SVG 图标

### 4.3 「目标」区

- label：`text-[11px] font-medium uppercase tracking-wide text-neutral-400`，文案「目标」
- 只保留**文件夹下拉**（worktree / 分支本轮不做）
- 下方一行说明：`text-[11px] text-neutral-400`「任务将在该文件夹中创建会话并执行。」
  （替换 codeg 的「任务从该分支拉出工作树…」，因为本项目没有 worktree）

### 4.4 页脚

- 右侧：`取消`（ghost/描边）、`保存`（实心 `bg-neutral-900 text-white`，
  `disabled` 当校验不过或在保存中），`gap-2` 右对齐
- **不渲染模板按钮**：模板功能本轮不实现，也不放禁用占位

> codeg 页脚左侧有「模板」popover（列表 + 「把当前内容存为模板」+ hover 删除）。
> 该能力需要模板存取的数据层，不在本轮范围；移除按钮而不是给一个点不动的假按钮。

### 4.5 校验

标题非空 → 提示词非空 → 项目必选 → 引擎必选。错误文案显示在页脚上方 `text-[11px] text-red-600`。

## 5. 兼容 / 回滚

- 改动全部集中在 `src/views/tasks/`：`boardColumns.ts`、`TasksView.vue`、`TaskColumn.vue`、
  `TaskCard.vue`、`TaskEditorDialog.vue`、新增 `EngineMark.vue`、`taskChip.ts` 微调。
- **无 Rust 改动、无 service / store / types 改动。**
- 回滚 = `git checkout` 上述文件，并删除 `EngineMark.vue`。

## 6. 风险

| 风险 | 缓解 |
|---|---|
