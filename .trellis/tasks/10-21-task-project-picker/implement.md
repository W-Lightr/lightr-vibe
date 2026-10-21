# Implement: 待办任务项目选择页与侧栏文件夹折叠（父任务）

两个子任务互不依赖。建议顺序：1（小、独立）→ 2（改路由）。

## 验证命令

```powershell
pnpm vue-tsc --noEmit
pnpm build
cargo check                 # workdir: src-tauri（本轮不应有 Rust 改动）
```

## 1 · 子任务 `10-21-sidebar-folder-collapse`

- [x] `SidebarPanel.vue`：
  - [x] `collapsedProjects: ref<Set<string>>`，localStorage key `lightr.sidebar.collapsedProjects`
        （JSON 数组，setup 同步读取 + try/catch）
  - [x] 文件夹行左侧加 `ChevronRight`，收起朝右 / 展开旋转 90°
  - [x] **箭头单独点击** → `toggleCollapse(projectId)`；整行点击仍是 `openProject`
  - [x] 收起时不渲染会话列表；计数与悬浮按钮（新建会话 / 移除项目）保留
  - [x] 收起且该项目有生成中会话 → 计数旁蓝点
  - [x] `query` 非空时忽略折叠（搜索能搜到收起项目），并显示匹配数
- [x] 确认：空项目折叠无异常；移除项目后其折叠记录不影响其它项目

**回滚点**：`git checkout src/views/coding/components/SidebarPanel.vue`

## 2 · 子任务 `10-21-task-project-picker-ui`

- [ ] 新增 `src/views/tasks/ProjectPickerView.vue`：
  - [ ] 搜索框（项目名 + rootPath，大小写不敏感）
  - [ ] 项目卡片网格（`sm:grid-cols-2 lg:3 xl:4`），卡片 = 图标 + 项目名 + 任务数 + 四列计数
  - [ ] 「全部项目」卡片固定最后，进入 `/tasks/all`
  - [ ] 空状态：无项目 → 提示 + 「打开文件夹」（复用 `fsService.pickFolder` + `projectStore.addProject`）
  - [ ] 进入时 `await taskStore.load()`（幂等）
  - [ ] 顶部「新建任务」→ 编辑器 `defaultProjectId = null`
- [ ] `router/index.ts`：
  - [ ] `/tasks` → `ProjectPickerView`
  - [ ] `/tasks/:projectId` → `TasksView`（`props: true`）
  - [ ] 保留 `/tasks/all` 走 `TasksView`（`projectId = 'all'`）
- [ ] `TasksView.vue`：
  - [ ] 改为读 `props.projectId`（`'all'` = 全部项目），移除顶部项目下拉
  - [ ] `watch(projectId)` 重置 `showCanceled/showArchived` 之外的瞬态（拖拽、在途集合）
  - [ ] 项目不存在（非 `'all'`）→ 「项目已移除」+ 返回项目选择页
  - [ ] 「新建任务」传 `defaultProjectId = projectId`（`all` 时为 null）
  - [ ] 返回按钮回到 `/tasks`
- [ ] `TaskEditorDialog.vue`：`defaultProjectId` 非空时预选，且在创建模式下**锁定**该项目

**回滚点**：删除 `ProjectPickerView.vue`；`git checkout src/router/index.ts src/views/tasks/TasksView.vue src/views/tasks/components/TaskEditorDialog.vue`

---

## 关键约束

- 不改数据层（`types/task.ts`、`services/`、`stores/`、`src-tauri/`）。
- `lightr.tasks.projectFilter` 退役：读到旧值忽略即可，不做迁移。
- 颜色只用 `neutral-*` + `blue-*` + 语义色；卡片无阴影，hover 只变边框色。
- PowerShell `Get-Content`/`Set-Content` 会破坏 UTF-8 中文，用 edit/write 工具或 Node/Python。

## 人工验收

- [ ] 收起两个文件夹 → 刷新应用 → 仍是收起的
- [ ] 收起后搜索会话 → 能搜到收起的项目里的会话
- [ ] 侧栏「待办任务」→ 项目卡片页 → 点某项目 → 该项目四列看板
- [ ] 看板点「新建任务」→ 编辑器里项目已选中且不可改
- [ ] 项目卡片页「全部项目」→ 跨项目看板
