# Design: 真实文件夹项目管理（子任务 1）

需求见 `prd.md`。桥接规范：`.trellis/spec/frontend/tauri-bridge.md`。

## 1. 架构与文件面

```
src-tauri/src/fs.rs            新：fs_list_dir / fs_read_file / fs_write_file
src-tauri/src/lib.rs           + mod fs + handler 注册
src-tauri/Cargo.toml           无新 crate（std::fs 足够）
capabilities/default.json      + tauri-plugin-dialog 的 allow-open（仅目录）
package.json                   + @tauri-apps/plugin-dialog
src/types/project.ts           Project/DirEntry 类型（从 types/session.ts 迁出 Project）
src/stores/project.ts          真实化：rootPath 形状 + 持久化 + actions
src/services/projectMock.ts    浏览器 mock（内存树/读写）
src/services/fsService.ts      isTauri 双通道封装（list/read/write/pick）
src/views/coding/components/
  SidebarPanel.vue             按钮接线 + 「最近」分组
  FilesTree.vue                懒加载重写
  FileEditorView.vue           dirty/保存/冲突
src/stores/workspace.ts        + dirtyFileTabs: Set<string>（tabId）
```

## 2. 契约

Rust（camelCase serde）：
- `fs_list_dir { path } -> Result<{ entries: DirEntry[] }, String>`
  DirEntry = `{ name, type: 'dir'|'file', size?, mtimeMs? }`，dirs-first，
  名称 locale 排序；`.git` 节点显示但 `entries` 返回空（内容不列，防万级节点）。
- `fs_read_file { path } -> Result<{ text, mtimeMs }, String>`
  错误串约定：`too-large`（>2MB）/ `binary`（含 NUL 字节探测）/ `not-found` /
  `outside-project` / io 消息。
- `fs_write_file { path, text, expectedMtime } -> Result<{ mtimeMs }, String>`
  `conflict` 语义同 tauri-bridge；**无 .bak**（D3）；临时文件 rename 原子写。
- 项目边界：fsService 在调用前校验 path 以某已打开项目 rootPath 为前缀
  （前端校验为主，Rust 侧仅做规范化防 `..` 逃逸出 rootPath）。

前端 pick：`import { open } from '@tauri-apps/plugin-dialog'`，
`open({ directory: true, multiple: false })` → string|null。

持久化：`~/.lightr-vibe/lightr-vibe/projects.json` =
`Project[] { id, name, rootPath, addedAt, lastOpenedAt }`。
读写经 Rust 新命令 `fs_read_json/write_json { path }`？→ 不：复用 agent.rs 的
通用文本读写不合适（mtime 语义不同）。新增 `fs_read_text { path } ->
Result<Option<String>,String>` 与 `fs_write_text { path, text }`（无 mtime 校验，
应用自有文件），projects.json 与子任务 2 的索引共用。

## 3. 决策

- **D1 目录对话框走前端 dialog 插件**：Rust 侧无跨平台原生对话框实现；插件
  官方维护。capabilities 仅加 `dialog:allow-open`（最小授权，符合 bridge spec
  「能不扩不扩」的例外：此处无 std 替代）。
- **D2 懒加载而非全树**：真实仓库全树可达数万节点；展开时请求 + 缓存。
- **D3 用户源码不备份**：.bak 会污染用户 git 仓库 status；conflict 保护已足够
  （与 agent.rs 对用户配置文件的 .bak 策略不同——那是单文件凭据配置）。
- **D4 移除项目不级联删会话**：会话 projectId 置空归入「聊天」组，历史不丢。
- **D5 dirty 基线 = 加载时 mtime + text**：保存后双基线同更新；外部修改仅靠
  mtime 冲突检测（不做内容 diff 提示）。

## 4. 兼容/回滚

- mock 通道保持旧 `Project{root,files}` 形状于 projectMock，组件经 fsService
  统一消费 → 组件不感知双形状。
- 回滚点：SidebarPanel 按钮行、FilesTree/FileEditorView 单文件可独立还原。

## 5. 测试要点

- 越界：path 含 `..` 逃逸 → outside-project。
- 二进制/大文件错误串前端映射为中文提示。
- 冲突：外部 touch 改 mtime 后保存 → conflict 分支。
- 持久化往返：add → 重启进程（mock 层单测式脚本）→ 列表一致。
