# Tauri Bridge Guidelines

> Cross-layer contracts between the Vue frontend and the Rust shell
> (`src-tauri/src/`). Established by task `10-08-settings-agent-mgmt`
> (`src-tauri/src/agent.rs`). Any new `#[tauri::command]` or frontend
> environment-dependent code must follow this spec.

---

## Scenario: Adding a Rust command + frontend dual-channel service

### 1. Scope / Trigger

Applies when a change includes any of:

- New/changed `#[tauri::command]` signature in `src-tauri/src/`.
- Frontend code that behaves differently in Tauri vs browser dev server.
- Reading/writing user-owned files outside the app bundle (configs, env files).
- Spawning external binaries (agent CLIs) from Rust.

### 2. Signatures

Rust side (`src-tauri/src/agent.rs`, registered in `lib.rs` `invoke_handler`):

```rust
#[tauri::command]
pub async fn agent_detect_opencode() -> DetectResult            // spawn_blocking 内跑外部命令

#[tauri::command]
pub fn agent_read_opencode_config() -> Result<ConfigReadResult, String>

#[tauri::command]
pub fn agent_write_opencode_config(
    text: String,
    expected_mtime: Option<u64>,   // JS 侧 camelCase: expectedMtime
) -> Result<ConfigWriteResult, String>   // Err("conflict") 为约定错误串

#[tauri::command]
pub fn agent_read_env_file() -> Result<EnvFileResult, String>

#[tauri::command]
pub fn agent_write_env_file(text: String) -> Result<(), String>
```

Structs use `#[serde(rename_all = "camelCase")]` so TS interfaces match field
for field (`binPath`, `mtimeMs`, `acpOk`, `configExists`). Keep that attribute
on every returned struct — TS types are written camelCase.

Frontend side: **all `invoke()` calls live in exactly one service module**
(`src/services/opencodeService.ts`), each guarded by `isTauri()`
(`src/utils/isTauri.ts`, checks `window.__TAURI_INTERNALS__`); the browser
branch delegates to a sibling `*Mock.ts` module with the same return shape.
Components and stores never call `invoke` directly.

### 3. Contracts

- **Detect**: `{ installed, version: string|null, binPath: string|null,
  platform, acpOk, configPath, configExists, detail: string|null }`.
  Never throws — failures are encoded in fields (`installed:false` + `detail`).
- **Config read**: `{ text: string|null, mtimeMs: number|null, path }`.
  Missing file → `text:null`, not an error.
- **Config write**: caller sends the **full patched text** (JSONC patch is
  produced frontend-side by `src/utils/jsonc.ts` `patchConfig`, which wraps
  `jsonc-parser.modify()` — minimal edits, comments/format preserved). Rust only
  validates mtime, backs up, and writes. Returns `{ mtimeMs }` of the new file.
- **Env file**: `~/.lightr-vibe/agent-env.json` (`{ "vars": { K: V } }`),
  directory auto-created. App-level only — never inject keys into user agent
  configs (official opencode schema has no global `env` object).
- Secrets: API keys are masked for display (`maskApiKey`: first 5 + last 2);
  plaintext lives only in an in-memory field, never logged, never in templates
  unless the user toggles "显示".

### 4. Validation & Error Matrix

| Condition | Behavior |
|---|---|
| `expectedMtime` ≠ on-disk mtime (or existence mismatch) | `Err("conflict")`; frontend shows 「配置已被外部修改」 and reloads; **never overwrite** |
| External binary not on PATH | detect returns `installed:false`; UI shows install guidance, no config editors |
| Direct spawn fails on Windows (`.cmd`/`.bat` shim, e.g. version-fox) | Fallback `cmd /C <bin> <args>` with `CREATE_NO_WINDOW`; timeout does not re-fallback |
| Command exceeds 5s | Kill child, mark that precheck `fail` with `detail: timeout...`; stdout/stderr drained on **separate threads** so a full pipe cannot deadlock the wait loop |
| Write I/O error | `Err(msg)`; store keeps edit state, `writing` lock released in `finally` |
| Browser dev server | `isTauri()` false → mock channel; UI shows 「浏览器模式：数据为 mock」 banner |

### 5. Good/Base/Bad Cases

- **Good**: store action → service (isTauri branch) → invoke → Rust std-only
  command → typed camelCase result → store mutation → component re-render.
- **Base**: read-modify-write cycle carries `mtimeMs` from the read through to
  the write's `expectedMtime`.
- **Bad**: component calls `invoke` directly; Rust command grows business logic
  (JSON parsing/patching belongs frontend); adding `tauri-plugin-fs` or new
  `capabilities` entries to reach user files (std::fs inside a command needs no
  permission and keeps the allowlist empty).

### 6. Tests Required

- **JSONC roundtrip** (assertion points): patch one key on a real config copy →
  diff shows exactly that key's line changed; comments/key order/indent/CRLF
  preserved; new nested key (`permission.bash`) insertable. Script:
  `.trellis/tasks/10-08-settings-agent-mgmt/roundtrip-test.cjs` (read-only
  against the real file — safe to re-run).
- **Conflict matrix**: four quadrants (mtime Some/None × file exists/absent)
  each produce the documented result.
- **Backup retention**: after >5 writes only newest 5 `.bak-<ms>` remain
  (13-digit ms names sort lexicographically = chronologically).
- **Build gates**: `pnpm build` (vue-tsc) + `cd src-tauri && cargo check`.

### 7. Wrong vs Correct

#### Wrong

```ts
// 组件里直接 invoke，且用显示名定位 provider（name ≠ 配置键时会串写凭据）
await invoke('agent_write_opencode_config', { text: patched })
saveApiKey(provider.name)   // "火山引擎" 这类 name 可能不是对象键
```

#### Correct

```ts
// service 层双通道；provider 视图携带配置键 key，补丁路径用 key
if (isTauri()) await invoke('agent_write_opencode_config', { text, expectedMtime })
else await mock.writeConfig(text, expectedMtime)
saveApiKey(provider.key)
```

---

## Filesystem commands (`src-tauri/src/fs.rs`, task 10-08-real-folder-projects)

Signatures (camelCase results, `Result<_, String>` with fixed error strings):

```rust
fs_list_dir { path, rootPath }  -> { entries: DirEntry[] }   // dirs-first; .git lists empty
fs_read_file { path, rootPath } -> { text, mtimeMs }         // too-large(>2MB) / binary(NUL)
fs_write_file { path, text, expectedMtime, rootPath } -> { mtimeMs }  // conflict / atomic
fs_read_text { path }  -> Option<String>                     // app-owned files, no mtime gate
fs_write_text { path, text }                                 // auto-creates parent dirs
```

- Error string vocabulary (frontend maps to Chinese via `FS_ERROR_MESSAGES`):
  `too-large | binary | not-found | outside-project | conflict`. Both the Rust and
  the mock channel must be able to produce every string — keep them in sync.
- Boundary: every project-scoped command takes `rootPath`; Rust `ensure_inside`
  normalizes `..` lexically then compares **by path components** (`starts_with`),
  so `/rootEvil` never matches `/root`. Frontend re-checks with a trailing-slash
  prefix before invoking.
- User source files get **no `.bak`** (would pollute the user's git status);
  conflict protection + atomic tmp+rename is the guarantee. `.bak` remains only
  for the single-file opencode config in `agent.rs`.
- Atomic write must clean the tmp file on **both** failure branches (write and
  rename).

## App data namespace (hard rule)

`~/.lightr-vibe/` root already contains **another application's** data
(`projects.json`, `sessions.json`, `messages/`, …). Lightr-vibe must only write
under `~/.lightr-vibe/lightr-vibe/` (see `appNamespaceDirSync()` in
`src/utils/appData.ts`). Never read or write the root-level files.

## Native dialogs

Directory picking uses `@tauri-apps/plugin-dialog` from the **frontend**
(`open({ directory: true })`, dynamic import inside the isTauri branch only).
Capability surface added for it: `dialog:allow-open` — nothing else. Rust has no
cross-platform native dialog; do not re-implement picking in Rust.

## ACP session events (`src-tauri/src/acp.rs`, task 10-08-opencode-acp-chat)

- Transport: **newline-delimited JSON-RPC 2.0 over stdio** (no Content-Length).
  One `writeln!` of compact JSON per message; blank/non-JSON lines are skipped.
- Rust routes stdout lines three ways: response (has `id` **and** no `method`) →
  pending oneshot; notification (`method`, no `id`) → `app.emit`; client request
  (`method` + `id`) → `request_permission` forwarded to the UI,
  `fs/*` / `terminal/*` / `elicitation/*` refused with `-32601`.
- **JSON-RPC ids are preserved as raw `Value`** — agent may legitimately use
  string ids. Never model an incoming id as `u64`, and never treat `id: null`
  as a response.
- Events emitted: `acp://update` `{sessionId, update}` · `acp://permission`
  `{requestId, sessionId, toolCall, options}` · `acp://status` `{status, reason}`.

> **Critical gotcha**: the `sessionId` on ACP events is the **agent-side** id
> (`ses_xxx` from opencode), while local sessions use `createId('ses')` ids
> (`ses-xxxxxxxx`). Every consumer must map through
> `sessionStore.findByAcpSessionId()` before touching local state. Skipping this
> mapping silently drops all streaming updates and makes permission cards never
> appear (both were real bugs found in review).

Other ACP behaviors worth keeping:

- `tool_call_update.content` is **cumulative replacement** — overwrite, never append.
- `session/load` replays history as `session/update` before responding; suppress
  those updates for that session or the local transcript doubles.
- `session/cancel` is a notification (no response awaited); idle cancel is a
  safe no-op; cancel must also resolve that session's pending permissions.
- Permission option selection must match by `kinds` (`allow_once` before
  `allow_always`), never by array position.
- Timeouts: initialize 10s / `session/new` 30s / `prompt` 300s / other 30s.
  The first `session/new` on a cold agent takes 8–13s (model enumeration).
- Always `create_dir_all(cwd)` before `session/new` — the protocol requires an
  existing absolute cwd.

### Process spawn (`build_acp_command`)

Both the long-lived connection (`start_inner`) and the one-shot probe
(`probe_options_inner`) build the child through **one** helper,
`build_acp_command()` (`resolve_spawn()` → real exe or `cmd /C opencode acp`,
piped stdio, `CREATE_NO_WINDOW` on Windows). It **removes
`OPENCODE_SERVER_USERNAME` / `OPENCODE_SERVER_PASSWORD`** from the child env.

> **Gotcha**: those two vars are exported by *another* running opencode instance
> and leak into our spawned `opencode acp`. With them present, opencode's free
> Zen models fail upstream with `acp:-32603 ... Invalid credential`. Removing
> them restores the clean, credential-less state codeg gets by launching
> `opencode acp` independently. Only these two are stripped; `OPENCODE`,
> `OPENCODE_PID` and user-provider keys must pass through.

### One-shot model probe (`acp_probe_options`)

`acp_probe_options(cwd?)` spawns a **throwaway** `opencode acp`, runs
`initialize` → `session/new`, returns `configOptions`, then closes stdin and
reaps the child. It never touches the global `conn_slot`, never emits `acp://*`,
and must reap on **every** path (a real timeout via a reader thread + `mpsc`,
not a blocking `read_line`). The frontend caches the result per agent and calls
it on session open / manual refresh so the model dropdown has real values before
the first prompt. The one-shot reader must match a response by **id AND absence
of `method`** (an agent→client request can collide on id).

### Model context window (`agent_model_context_window`)

`session/set_config_option` returns only `configOptions`, **never** the model's
`limit.context` (the context-window denominator). So the composer resolves it
locally, mirroring codeg's precedence:

1. **User config** — `$OPENCODE_CONFIG`, then `$XDG_CONFIG_HOME/opencode` else
   `$HOME/.config/opencode`, reading `config.json` / `opencode.json` /
   `opencode.jsonc` (JSONC tolerated): `provider|providers.<pid>.models.<mid>.limit.context`.
2. **models.dev cache** — `$OPENCODE_MODELS_PATH` else
   `$XDG_CACHE_HOME|$HOME/.cache` + `/opencode/models.json`:
   `<pid>.models.<mid>.limit.context`.

`limit.context` accepts int or float (>0 else absent). Returns `Ok(None)` for
everything not found — never errors the UI. The model value is `provider/modelId`
split on the **first** `/` only (the modelId itself may contain `/`, e.g.
`magpie/workbuddy-ai/deepseek-v4.1-flash`). On model switch the composer writes
the resolved window to `tokenUsage.contextWindow` immediately (best-effort,
never blocks); the live `usage_update.size` still refines it per turn.

## Gotchas

> **Warning**: `getCurrentWindow()` throws in a plain browser dev server.
> Use `getAppWindow()` (`src/utils/window.ts`, returns `null` off-Tauri) and
> hide window controls when null. `TopBar.vue` still calls `getCurrentWindow()`
> directly (pre-existing) — migrate it when touching that file.

> **Warning**: Windows PATH shims (`.cmd`, e.g. version-fox/volta/fnm) are not
> executable via `CreateProcess`. Any new external-binary spawn needs the
> `cmd /C` fallback chain from `agent.rs::run_probe`.

> **Warning**: `opencode` config keys are schema-versioned; top-level small
> model is `small_model` (snake_case), not `smallModel`. Verify against
> https://opencode.ai/config.json before writing new keys.
