# Implement: 真实文件夹项目管理（子任务 1）

上下文顺序：`prd.md` → `design.md` → `.trellis/spec/frontend/tauri-bridge.md`
→ ui-style/component/state/type spec。

## 有序清单

### A. Rust 与依赖
1. `src-tauri/src/fs.rs`：fs_list_dir / fs_read_file / fs_write_file /
   fs_read_text / fs_write_text（契约见 design §2；错误串约定；.git 空内容；
   2MB/NUL 探测；原子写；`..` 规范化）。
2. `lib.rs` 注册；`cargo check`。
3. `pnpm add @tauri-apps/plugin-dialog`；Rust `tauri-plugin-dialog` 依赖 +
   `lib.rs` plugin 注册；`capabilities/default.json` 追加 `dialog:allow-open`。

### B. 前端基础层
4. `src/types/project.ts`（Project/DirEntry）；`types/session.ts` 移除旧
   Project/FileNode（引用点同步改）。
5. `src/services/fsService.ts`（isTauri 双通道 + 项目边界校验 + pick 封装）。
6. `src/services/projectMock.ts`（旧 mock 形状适配为 DirEntry 树内存实现）。
7. `src/stores/project.ts` 真实化（持久化经 fs_read_text/fs_write_text；
   actions：addProject/removeProject/touchProject；启动 load）。
8. `main.ts`：mount 前 `await projectStore.load()`（initAppData 之后）。

### C. UI
9. `SidebarPanel.vue`：按钮 @click → pick → addProject；「最近」分组
   （lastOpenedAt 倒序 5 项，折叠头样式同现有分组）；浏览器禁用态。
10. `FilesTree.vue` 懒加载重写（展开请求/缓存/loading spin/错误重试/空占位）。
11. `FileEditorView.vue`：真实读取 + dirty 基线 + 保存按钮与 Ctrl+S +
    conflict 二选一（重新加载/覆盖）+ message 反馈。
12. `workspace.ts` + `SessionTabs.vue`：dirtyFileTabs 圆点（文件标签标题前
    amber-400 圆点，ui-style 语义色规则）。

### D. 验证
13. `pnpm build` + `cargo check`。
14. Tauri 实机：AC1-6（打开/懒加载/保存/冲突/重启/移除/大文件）。
15. 浏览器 mock：AC7。
16. 命名空间验证：AC8（既有文件 mtime 不变）。
17. trellis-check 后提交。

## 验证命令

```bash
pnpm build && cd src-tauri && cargo check
pnpm dev:tauri   # AC2-6
pnpm dev         # AC7
```

## 风险/回滚

- capabilities 首次扩项（dialog:allow-open）：回滚 = 删该行 + 前端 pick 改禁用。
- 用户文件写路径：conflict 保护 + 原子写；无 .bak（design D3）。
- FilesTree/FileEditorView 为整文件重写：git 可单文件还原。

## start 前检查
- [ ] implement.jsonl/check.jsonl 已填
- [ ] 父任务 prd 决策已并入（Q3/命名空间）
- [ ] 用户批准最终摘要
