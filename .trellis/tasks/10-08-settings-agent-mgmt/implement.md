# Implement: 设置界面与 Agent 管理（OpenCode 集成）

任务：10-08-settings-agent-mgmt。上下文顺序：`research/opencode-integration.md`
→ `prd.md` → `design.md`。规范：`.trellis/spec/frontend/*`（尤其 ui-style.md、
component-guidelines.md、state-management.md、type-safety.md）。

## 有序实施清单

### A. Rust 侧（先做，前端可并行 mock 开发）
1. `src-tauri/src/agent.rs`：
   - `agent_detect_opencode`：PATH 解析（opencode / opencode.cmd / opencode.exe）
     → spawn `--version`（失败回退 `cmd /C`）→ spawn `acp --help`（退出码）
     → 返回 `{ installed, version, binPath, platform, acpOk }`；5s 超时/命令。
   - `agent_read_opencode_config`：解析 `~/.config/opencode/opencode.jsonc`
     （Windows: `%USERPROFILE%\.config\opencode\opencode.jsonc`）→ `{ text, mtimeMs, path }`；
     不存在 → `{ text: null, ... }`。
   - `agent_write_opencode_config { text, expectedMtime }`：mtime 校验 →
     备份 `.bak-<ts>`（清理保留 5 份）→ 原子写（写临时文件后 rename）→ 新 mtime。
   - `agent_read_env_file` / `agent_write_env_file`：`~/.lightr-vibe/agent-env.json`，
     目录自动创建。
2. `lib.rs`：`mod agent;` + `invoke_handler` 追加 5 个命令。
3. 验证：`cd src-tauri && cargo check`。

### B. 前端基础层
4. `pnpm add jsonc-parser`。
5. `src/types/agent.ts`：AgentDef、PrecheckItem、DetectResult、OpencodeConfigView、
   ProviderView、ModelView、PermissionAction（字符串字面量联合，无 enum）。
6. `src/utils/jsonc.ts`：`patchConfig(text, edits: {path:(string|number)[], value:unknown}[])`
   → 应用 jsonc-parser `modify` 编辑（eol 按文本检测 CRLF/LF，tabSize 2）。
7. `src/services/opencodeMock.ts` + `src/services/opencodeService.ts`
   （isTauri 双通道；写锁在 store）。
8. `src/stores/agent.ts`：setup store —— detect 状态、prechecks、configView、
   envVars、enabled、writing 锁、actions（detect/refresh/saveModel/savePermission/
   saveEnv）。

### C. 设置页 UI
9. `src/views/settings/SettingsView.vue`：拖拽条 + 自绘窗口控件（抄 HomeView
   模式）+ 左菜单 + 内容区；返回按钮 `router.back()` 兜底 `/`。
10. `SettingsMenu.vue`：菜单项数组（Agent 管理真实 + 设计图其余条目禁用态，
    点击 `message.info('「X」即将推出')`）。
11. `AgentManageView.vue` + `AgentListItem.vue`：OpenCode 行（lobehub
    `opencode.svg` 内联）+ 设计图其余 Agent 禁用行（claudecode/codex/gemini/
    cline/cursor 图标）；状态点 + PASS/FAIL 徽章 + 刷新。
12. `OpencodeDetail.vue`：头（图标/名称/binary 徽章/描述/启用开关）+
    未安装引导分支 + 浏览器 mock 提示条分支。
13. `PrecheckPanel.vue`：4 预检项折叠行（chevron + label + PASS/FAIL 徽章）+
    「预检项：N」+ 诊断按钮（spin 态）。
14. `ModelConfigCard.vue`：主/小模型 NSelect（选项 = providers×models 的
    `provider/model`）+ Provider 列表卡（名称/npm/baseURL/模型数/apiKey 掩码
    + 显示切换）+「连接 Provider」占位 +「刷新目录」。
15. `PermissionCard.vue`：schema 15 键 × ask/allow/deny 三态（NSelect，含
    「默认（继承）」空态）。
16. `EnvVarsCard.vue`：KEY=VALUE 行列表 + 增删 +「保存环境变量」黑底按钮
    （设计图样式：`bg-neutral-900 text-white rounded-md`）。
17. 入口接线：`router/index.ts` + `/settings`；`TopBar.vue:117` 齿轮改
    `router.push('/settings')`；`HomeView.vue` 右上角齿轮按钮。

### D. 验证与收尾
18. JSONC 往返测试（手工脚本）：读真实 `opencode.jsonc` → patchConfig 改
    `model` → 与原文 diff 确认仅目标键变化、注释保留。
19. `pnpm build`；`cd src-tauri && cargo check`。
20. `pnpm dev:tauri` 手动验证 AC2-AC6；`pnpm dev` 验证 AC7（mock 提示条）。
21. trellis-check 全量检查后提交。

## 验证命令

```bash
pnpm build                      # vue-tsc --noEmit && vite build
cd src-tauri && cargo check     # Rust 编译检查
pnpm dev                        # 浏览器 mock 通道
pnpm dev:tauri                  # 真实通道（AC2-6）
```

## 风险文件 / 回滚点

- `~/.config/opencode/opencode.jsonc`：用户真实凭据文件 —— 只经带备份+mtime
  校验的写路径；测试写入前先复制备份到任务目录外。
- `src-tauri/src/lib.rs`：仅追加；回滚 = 删除 mod 行与 handler 注册。
- `TopBar.vue` 齿轮行：唯一行为变更；回滚 = 还原 `notImplemented('设置')`。
- `router/index.ts`：纯追加路由。

## start 前检查

- [ ] implement.jsonl / check.jsonl 已填真实 spec 条目
- [ ] prd/design/implement 三件齐备且与最新决策一致
- [ ] 用户已批准最终规划摘要
