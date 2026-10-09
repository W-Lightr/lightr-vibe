# 子任务 1：真实文件夹项目管理

父任务：`10-08-real-agent-workspace`。本任务可独立规划/实现/归档。
顺序：本任务先于 `10-08-opencode-acp-chat`（后者需要真实 cwd，但可先用
tmpsession 目录并行先行，不硬阻塞）。

## Goal / 用户价值

修复用户报告的 bug（侧栏「打开文件夹」按钮无响应），并把文件夹项目从 mock
内存文件系统升级为真实磁盘目录：原生对话框选目录 → 真实目录树懒加载浏览 →
Monaco 打开/保存真实文件 → 项目跨启动持久化（侧栏「最近」分组）。

## Confirmed facts

- Bug 根因：`src/views/coding/components/SidebarPanel.vue:196-200` 按钮无
  `@click` 绑定；项目数据源为 `src/stores/project.ts` 的 `mockProjects`。
- `FileEditorView.vue:31`：Monaco onChange 仅写内存（`setFileContent`），无保存
  动作、无 dirty 指示、无冲突处理。
- `FilesTree.vue`：一次性渲染 `project.root` 全树（mock 形状），双击打开文件。
- Tauri 现状：无 dialog/fs 插件；capabilities 仅 sidecar shell 权限。
- 桥接规范：`.trellis/spec/frontend/tauri-bridge.md`（camelCase 契约、conflict
  语义、原子写、isTauri 双通道、.cmd 回退链 gotcha）。
- 数据命名空间：父任务 prd 约束 —— 本项目数据只写
  `~/.lightr-vibe/lightr-vibe/`（既有 `~/.lightr-vibe/projects.json` 等属其他
  应用，禁止触碰）。
- 用户决策（父任务 Q3）：项目跨启动持久化 + 「最近」分组。

## Requirements

### R1 打开文件夹入口
- SidebarPanel「打开文件夹」按钮接线：Tauri 环境调原生目录选择对话框
  （tauri-plugin-dialog，仅允许选目录）；浏览器 dev 环境按钮禁用态 +
  title 提示「桌面端可用」。
- 选择后：校验目录存在且可读 → 加入项目列表（id=`proj-<hash(path)>` 或
  createId('proj')，name=目录名，rootPath，addedAt/lastOpenedAt）→ 持久化 →
  侧栏立即出现并可展开。
- 重复打开同一路径：复用已有项目并更新 lastOpenedAt，不产生重复行。

### R2 Rust fs 命令（新模块 src-tauri/src/fs.rs）
- `fs_pick_folder() -> Option<PickedFolder>`：原生对话框（dialog 插件前端 API
  或 Rust side？→ 见 design D1：前端 `@tauri-apps/plugin-dialog` open()）。
  实际命令面：`fs_list_dir { path } -> DirEntry[]`（dirs-first 排序，含
  name/type/size/mtimeMs；隐藏文件默认过滤，dot 目录如 .git 跳过内容但显示节点？
  → 显示节点、展开时正常列；.git 内容跳过）、`fs_read_file { path } ->
  { text, mtimeMs }`（>2MB 拒绝 'too-large'；非 UTF-8 拒绝 'binary'）、
  `fs_write_file { path, text, expectedMtime }`（conflict/原子写/备份策略见
  design D3：用户源码文件**不做 .bak**，仅 conflict 保护，避免污染用户仓库）。
- 所有路径参数做规范化（去 `..` 越界不强制禁止——用户可能打开任意目录，但
  读写仅限已打开项目的 rootPath 子树，越界返回 'outside-project'）。

### R3 project store 真实化
- `Project` 形状改为 `{ id, name, rootPath, addedAt, lastOpenedAt }`；删除
  `root`/`files` 内存字段（mock 通道保留旧形状的 mock 实现于 projectMock.ts）。
- 持久化：`~/.lightr-vibe/lightr-vibe/projects.json`（数组），启动时加载
  （main.ts initAppData 之后、mount 之前 await）。
- actions：addProject/removeProject/touchProject；removeProject 仅删索引
  **不删磁盘文件**；删除时同步清理其会话的 projectId 引用（会话保留为孤儿
  临时会话？→ 保留会话但 projectId 置空，归入「聊天」组，design D4）。
- 「最近」分组：侧栏新增「最近」折叠组，列 lastOpenedAt 倒序前 5 个项目
  （父任务 Q3）。

### R4 FilesTree 懒加载
- 目录节点展开时才 `fs_list_dir`；loading 态 spin 图标；错误态（权限/不存在）
  行内红色提示 + 重试按钮；空目录显示「空」占位。
- 节点缓存于组件/store（展开过不重复请求，除非手动刷新按钮）。

### R5 文件编辑与保存
- 打开文件：`fs_read_file` 真实内容进 Monaco（languageByPath 映射沿用）。
- dirty 跟踪：内容偏离加载基线 → 标签页标题加圆点（workspace store
  `dirtyFileTabs`）；保存（按钮 + Ctrl+S）→ `fs_write_file` 带基线 mtime →
  成功清 dirty 更新基线；conflict → message 提示「文件已被外部修改」+
  「重新加载/覆盖」二选一（naive-ui dialog 或 message action）。
- 保存反馈：message.success('已保存')。

### R6 回归与双通道
- 浏览器 dev：projectMock 提供 mock 项目/树/读写（内存），设置页/会话链路不受
  影响；mock 提示条沿用既有模式。
- 不破坏设置页、Agent 管理、会话 mock 流（conversationService 不动）。

## Acceptance Criteria

1. `pnpm build` + `cargo check` 通过。
2. Tauri：点「打开文件夹」弹出原生目录对话框；选定后侧栏出现项目；展开目录
   为真实子目录（懒加载可见 loading）；双击文件 Monaco 显示真实内容。
3. 修改内容 → 标签出现 dirty 圆点 → Ctrl+S → 磁盘文件内容变化（外部 cat 验证）
   → 圆点消失；外部改文件后再保存 → 冲突提示不静默覆盖。
4. 重启应用：项目仍在侧栏；「最近」分组按 lastOpenedAt 排序。
5. 移除项目：侧栏消失、磁盘文件完好；其会话归入「聊天」组不丢失。
6. 打开 >2MB 或二进制文件：明确错误提示，Monaco 不崩溃。
7. 浏览器 dev：mock 通道全功能可用 + 提示条；「打开文件夹」按钮禁用态。
8. `~/.lightr-vibe/` 根下既有 projects.json/sessions.json/messages/ 未被读写
   （git 外验证：mtime 不变）。

## Out of Scope

- 文件新建/重命名/删除（树内）、拖拽移动、搜索过滤文件内容。
- git 状态标注、DiffViewer 增强。
- 多窗口/多工作区。
- ACP 会话（子任务 2）。
