# Design: 设置界面与 Agent 管理（OpenCode 集成）

任务：10-08-settings-agent-mgmt。需求见 `prd.md`，证据见 `research/opencode-integration.md`。

## 1. 架构与边界

```
src/
├── router/index.ts                     + /settings 顶层路由（懒加载）
├── views/settings/
│   ├── SettingsView.vue                全屏壳：拖拽条+窗口控件+左菜单+内容区
│   └── components/
│       ├── SettingsMenu.vue            左菜单（activeSection 受控）
│       ├── AgentManageView.vue         Agent 管理分区（列表+详情两列）
│       ├── AgentListItem.vue           列表行（图标/状态点/徽章/刷新）
│       ├── OpencodeDetail.vue          详情容器：头+预检+配置卡片编排
│       ├── PrecheckPanel.vue           预检项折叠列表 + 诊断按钮
│       ├── ModelConfigCard.vue         主/小模型下拉 + Provider 管理
│       ├── PermissionCard.vue          permission 三态编辑
│       └── EnvVarsCard.vue             KEY=VALUE 行编辑 + 保存
├── stores/agent.ts                     useAgentStore：检测状态/配置缓存/写锁
├── services/
│   ├── opencodeService.ts              检测+配置读写编排（Tauri/mock 双通道）
│   └── opencodeMock.ts                 浏览器 dev 回退数据
├── types/agent.ts                      AgentDef/PrecheckItem/OpencodeConfig 等
└── utils/jsonc.ts                      jsonc-parser 封装：patchConfig(text, edits)
src-tauri/src/
├── agent.rs                            检测/读写 commands（std::process + std::fs）
└── lib.rs                              + mod agent; invoke_handler 注册
```

边界原则：
- 组件只读 store / 调 service；service 是唯一知道 Tauri vs mock 的层
  （沿用 `appData.ts` 的 try-import 探测模式：`isTauri()` 工具函数）。
- Rust 命令保持"哑"：只做进程/文件操作与 mtime 校验，不含业务判断。
- JSONC 补丁在前端生成（jsonc-parser），Rust 只收最终文本 —— 避免 Rust 引
  JSONC 依赖，且补丁逻辑可被前端单测/往返测试覆盖。

## 2. 数据流与契约

### 2.1 检测流
`AgentManageView.onMounted` → `opencodeService.detect()` →
Tauri: `invoke('agent_detect_opencode')` →
返回 `{ installed, version, binPath, platform, acpOk, configPath, configExists }`
→ store 映射为 4 个 `PrecheckItem[]`（版本状态/Platform/配置文件/ACP 能力），
每项 `{ id, label, status: 'pass'|'fail'|'running'|'unknown', detail }`。
mock: 固定全 pass + version `1.18.35`。

Rust 检测顺序（research §1）：
1. `Command::new("opencode").args(["--version"])`（CreateProcess）
2. 失败 → `cmd /C opencode --version`（捕获 stdout 首行 semver）
3. `acp --help` 同链路探测（退出码 0 = acpOk）
4. binPath 由 `which` 语义解析：遍历 PATH 找 `opencode`/`opencode.cmd`/`opencode.exe`
超时 5s/命令，超时记 fail（detail 写明 timeout）。

### 2.2 配置读流
`invoke('agent_read_opencode_config')` → `{ text, mtimeMs, path }`；
前端 `jsonc-parser.parse(text)` → 归一化为 `OpencodeConfigView`：
`{ model, smallModel, providers: ProviderView[], permissions: Record<key, action> }`。
ProviderView = `{ name, npm, baseURL, apiKeyMasked, apiKeyPlain, models: ModelView[], whitelist }`。
ModelView = `{ id, label: 'provider/model', context?, reasoning?, toolCall? }`。

### 2.3 配置写流（patch 语义）
每次保存：`patchConfig(currentText, [{ path:['model'], value }, ...])`
→ jsonc-parser `modify()`（formattingOptions 取 `{ eol: 检测CRLF, insertSpaces:true, tabSize:2 }`）
→ 应用编辑得 newText → `invoke('agent_write_opencode_config', { text:newText, expectedMtime })`
→ Rust：mtime 不等 → `Err("conflict")`；等 → 备份（`.bak-<ts>`，清理 >5 份）→ 写 →
返回新 mtime。前端成功后更新 store 缓存的 text/mtime。
写锁：store 内 `writing: boolean`，重入直接 return + message.warning。

### 2.4 环境变量流
`agent_read_env_file` / `agent_write_env_file` 操作
`~/.lightr-vibe/agent-env.json`（`{ "vars": { "K": "V" } }`，纯 JSON 无注释问题）。
目录不存在则创建。浏览器 mock 存内存。

## 3. 关键决策与权衡

- **D1 检测走 Rust std::process，不改 capabilities**：前端 shell 插件白名单
  只能放固定参数且 `.cmd` shim 仍需 cmd 包装；Rust 侧可控超时/回退链且零权限面。
  代价：Rust 代码量略增（~150 行）。
- **D2 JSONC 补丁在前端**：jsonc-parser 是唯一成熟的最小编辑实现；Rust 无
  等价 crate 生态。代价：补丁正确性依赖前端测试（implement 阶段做真实文件
  往返 diff 验证）。
- **D3 环境变量落应用级文件**：官方 schema 无全局 env（research §3）；写进
  opencode.jsonc 会成为未知键（opencode 忽略但污染用户配置）。代价：语义需
  UI 文案解释（R6 已含）。
- **D4 设置分区用页内状态而非嵌套路由**：本期仅一个真实分区；嵌套路由会让
  禁用菜单项的路由守卫变复杂。后续多分区时再升级（菜单 id 已预留）。
- **D5 启用开关仅记录不生效**：避免"开关关了但会话仍走 mock"的伪功能争议；
  存 `useAgentStore.enabled`（内存 + 后续可落 agent-env.json 同级设置文件，
  本期内存即可，PRD R3 已声明）。

## 4. 兼容与回滚

- 不触碰现有 store/会话链路；`conversationService` mock 不变。
- TopBar 齿轮行为从 notImplemented 变为路由跳转 —— 唯一行为变更点，回滚即
  还原该行。
- Rust 新命令为纯新增；lib.rs 仅追加 mod + handler 注册。
- 用户配置文件的唯一写路径带备份与 mtime 校验；最坏情况用户从 .bak 恢复。

## 5. 依赖新增

- `jsonc-parser`（前端，MIT，VS Code 同源）。无其他新依赖。
- Rust 侧无新 crate（std 足够）。
