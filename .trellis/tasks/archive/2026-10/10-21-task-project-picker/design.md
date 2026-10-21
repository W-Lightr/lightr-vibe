# Design: 待办任务项目选择页与侧栏文件夹折叠

需求 `prd.md`。视觉规范 `.trellis/spec/frontend/ui-style.md`。

## 1. 侧栏文件夹折叠（子任务 1）

### 现状

`SidebarPanel.vue:303` 每轮渲染：
```html
<div v-for="group in folderGroups" :key="group.project.id" class="group/proj mb-2">
  <div class="flex h-7 cursor-pointer ..." @click="openProject(group.project.id)">
    <FolderOpen /> <span>{{ name }}</span> <span>{{ count }}</span>
    <button 新建会话 /> <NPopconfirm 移除 />
  </div>
  <!-- 会话列表（无条件渲染） -->
</div>
```

点击文件夹行目前是 `openProject`（展开该项目的第一个会话 / 新建）。**没有折叠概念。**

### 设计

**状态**：`collapsedProjects = ref<Set<string>>(...)`，键为 `projectId`。

- 持久化到 `localStorage`，key `lightr.sidebar.collapsedProjects`，值为 `string[]`（JSON）。
- setup 时同步读取（与 `TasksView` 的 `readBool` 同款 try/catch 容错）。

**交互**：

- 文件夹行左侧加 `ChevronRight`，收起时朝右、展开时旋转 90° 朝下（`transition-transform`）。
- **点击箭头区域** → 只切换折叠。
- **点击文件夹名/整行** → 保持现有 `openProject` 行为（打开该项目的会话）。
  > 理由：用户已经习惯点文件夹进项目；把整行改成折叠会破坏既有操作。箭头是新增的、
     无歧义的折叠入口，与 VS Code 的资源管理器一致。
- 收起的文件夹不渲染会话列表，但**保留**计数与「新建会话 / 移除项目」悬浮按钮。
- 收起状态下，若该项目有生成中的会话，计数旁显示一个蓝点（避免「有活在跑却看不见」）。

**空项目**：`folderGroups` 里 `sessions.length === 0` 的分组，收起/展开无视觉差异；
仍然可折叠（保持交互一致），收起后不显示「暂无会话」。

**搜索联动**：`query` 非空时**忽略折叠状态**，命中的会话一律显示（否则搜不到收起的项目），
并在该文件夹行显示「N 个匹配」。搜完清空后恢复折叠状态。

### 为什么不折叠「聊天」组

「聊天」是临时会话，没有项目 id，且通常条目少；给它单独的折叠状态需要额外键。
本轮不处理，保持常开。

## 2. 待办任务项目选择页（子任务 2）

### 路由

```
/tasks                      → ProjectPickerView.vue   （新增：项目卡片页）
/tasks/:projectId           → TasksView.vue           （现有看板，改成读路由参数）
```

- `TasksView` 不再依赖 localStorage 的 `lightr.tasks.projectFilter` 决定项目；
  改为 `props.projectId`（由路由参数传入）。
- 看板顶部原有的「项目下拉筛选」**移除**（项目已由路由确定）；保留「全部项目」视图
  作为可选项——通过选择页的「全部项目」卡片进入 `/tasks/all`。
  > 这样既满足「先选项目」，又没有剥夺跨项目总览能力。

### 项目卡片页布局

```
待办任务                                            [+ 新建任务]
┌ 搜索项目… ─────────────────────────────────┐
└──────────────────────────────────────────┘

┌──────────────┐ ┌──────────────┐ ┌──────────────┐
│ 📁 t3         │ │ 📁 t1        │ │ ⊞ 全部项目    │
│ 8 个任务      │ │ 0 个任务      │ │ 8 个任务      │
│ 待办 1 · 进行 0│ │ 待办 0 · …    │ │ 待办 1 · …    │
│ 等你 2 · 完成 5│ │              │ │              │
└──────────────┘ └──────────────┘ └──────────────┘
```

- 网格：`grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4`
- 卡片：`rounded-xl border border-neutral-200 bg-white p-3`，
  hover `border-blue-400`（与任务卡片同一语言），无阴影
- 卡片内容：
  - 首行：文件夹图标（`FolderOpen`，`text-neutral-400`）+ 项目名（`text-[13px] font-medium`，截断）
  - 第二行：`N 个任务`（`text-[11px] text-neutral-500`）
  - 第三行：四列计数徽标 `待办 1 · 进行中 0 · 等你处理 2 · 已完成 5`，为 0 的列灰显
- 「全部项目」卡片固定排在最后，图标用 `LayoutGrid`
- 空状态：没有项目 → `还没有打开任何文件夹` + 一个「打开文件夹」按钮（复用 `fsService.pickFolder`）
- 没有任务的项目仍然显示（`0 个任务`），点击进入看板后四列皆空

### 搜索

- 输入框 `h-8 rounded-md border border-neutral-200`
- 按项目**名**与 `rootPath` 做大小写不敏感包含匹配
- 无匹配：`没有匹配的项目`

### 新建任务默认项目

- 项目卡片页顶部「新建任务」按钮：打开编辑器时 `defaultProjectId = null`（需选项目）
- 看板页（`/tasks/:projectId`）的「新建任务」：`defaultProjectId = projectId`，编辑器里
  项目下拉**预选并锁定**（该任务必然属于此项目）
- 若用户从「全部项目」看板新建：仍需手动选项目

### 数据

- 项目列表：`projectStore.projects`
- 每个项目的任务计数：`taskStore.tasks` 按 `projectId` 聚合
  （在页面内 computed 一次，避免每张卡片各自过滤）
- 需要保证 `taskStore.load()` 已执行：进入选择页时 `await taskStore.load()`（幂等）

### 侧栏入口

`SidebarPanel.vue` 的 `navItems` 里 `todos.to = '/tasks'`（已是当前值），
无需改动——只是 `/tasks` 的目标组件换了。

## 3. 兼容 / 回滚

- 子任务 1：只改 `SidebarPanel.vue`。
- 子任务 2：新增 `views/tasks/ProjectPickerView.vue`；改 `router/index.ts`（`/tasks` 指向新页，
  加 `/tasks/:projectId`）；`TasksView.vue` 改为读路由参数、移除顶部项目下拉。
- `lightr.tasks.projectFilter` 这个 localStorage 键退役；读到旧值时忽略（不迁移、不报错）。
- 回滚：删除 `ProjectPickerView.vue`，路由与 `TasksView` 还原，`git checkout SidebarPanel.vue`。

## 4. 风险

| 风险 | 缓解 |
|---|---|
| 折叠点整行会破坏「点文件夹打开项目」的既有习惯 | 折叠只由箭头触发；整行行为不变 |
| 搜索时折叠项目搜不到 | `query` 非空时忽略折叠 |
| `/tasks/:projectId` 的项目被移除后页面空白 | 读不到项目 → 显示「项目已移除」+ 返回项目选择页 |
| 移除顶部项目下拉会让跨项目总览消失 | 保留「全部项目」卡片，对应 `/tasks/all` |
| 路由参数变化但组件复用导致状态不刷新 | `TasksView` 用 `watch(() => props.projectId)` 重置筛选与拖拽状态 |
