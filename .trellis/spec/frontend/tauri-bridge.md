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

**Process model: one `opencode acp` process + one connection PER SESSION** (aligned
with codeg; supersedes the earlier single long-lived process multi-session design).
Rust keeps `static CONNS: Mutex<HashMap<String, Arc<AcpConn>>>` keyed by **connId =
local session id** (`ses-xxxxxxxx`). `acp_connect(sessionId)` spawns+initializes the
session's own process (idempotent, per-session start lock); `acp_disconnect(sessionId)`
kills it (close stdin → ≤2s → kill). Closing a tab kills that session's process.
A background **idle sweep** thread (every ~15s) disconnects connections idle ≥
`LIGHTR_ACP_IDLE_TIMEOUT_SECS` (default 180, `0` = off) when not busy (no pending
request / pending permission).

- Transport: **newline-delimited JSON-RPC 2.0 over stdio** (no Content-Length).
  One `writeln!` of compact JSON per message; blank/non-JSON lines are skipped.
- Rust routes stdout lines three ways: response (has `id` **and** no `method`) →
  pending oneshot; notification (`method`, no `id`) → `app.emit`; client request
  (`method` + `id`) → `request_permission` forwarded to the UI,
  `fs/*` / `terminal/*` / `elicitation/*` refused with `-32601`.
- **JSON-RPC ids are preserved as raw `Value`** — agent may legitimately use
  string ids. Never model an incoming id as `u64`, and never treat `id: null`
  as a response.
- Events **carry `connId`** (local session id) so the frontend routes per connection:
  `acp://update` `{connId, sessionId, update}` · `acp://permission`
  `{connId, requestId, sessionId, toolCall, options, meta}` ·
  `acp://status` `{connId, status, reason}`.

> **Critical mapping rule**: `connId` is the **local** session id (`ses-xxxxxxxx`,
> matches the frontend `session.id`); `sessionId` is the **agent-side** id
> (`ses_xxx`, used only as the `session/*` method argument and for message
> attribution). Route updates/permissions by `connId` (`sessionStore.get(connId)`),
> NOT by `findByAcpSessionId` — the events no longer need that reverse lookup.
> A dead/stopped status affects **only that connId's** session (never clears all).

### Permission-card richness (`_meta.permission`, task 10-14-acp-permission-rich)

ACP permission requests carry agent-authored rich data the card must surface. The
Rust bridge **passes it through verbatim** — it never interprets it:

- **Request-level `_meta`** → the `acp://permission` event's `meta` field
  (`params._meta` cloned, `null` when absent). Holds
  `permission.{title, description, defaultToNo}` and `claudeCode.title`.
- **Option-level `_meta`** stays inside each entry of the `options` array
  (`PermissionOption.meta`) — `permission.{version:1, changes[]|description}`.
- **`toolCall.rawInput`** is forwarded as-is, except the bridge applies a
  best-effort `inject_raw_input_start_line_into` (no fs access): a `@@`-bearing
  JSON **string** is parsed to an object so the frontend diff view can read it;
  anything else is left untouched. It never throws.

**Frontend owner**: all decoding lives in **one** pure module,
`src/utils/permissionMeta.ts` (`parsePermissionTitle` / `parsePermissionDescription`
/ `isDefaultToNo` / `parsePermissionCommand` / `parseOptionChanges` / `extractDiff`
/ `optionLabel`). `PermissionCard.vue` must import these, never re-parse `_meta`
inline (code-reuse rule). Parsers are **strict**: `_meta.permission` is honoured
only when `version === 1`; `defaultToNo` counts only for the literal `true`;
anything unrecognised falls back to `toolCall.title` / the JSON summary / the
legacy three buttons (allow once / always / reject) — so an opencode request with
no `_meta` renders exactly as before (zero regression).

**Queue count is a frontend projection**: `useAcpPermission` already queues *every*
pending request per connection, so the card's「还有 N 个待审批」is
`permissionQueue.length - 1 - i` computed in `ConversationView.vue` — the Rust
`pending_perm` map is intentionally **not** turned into a server-side queue (only
needed for codeg's single-slot snapshot, which we do not have).

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
- Command surface (all take `connId`, camelCase params): `acp_connect`,
  `acp_disconnect`, `acp_status`, `acp_session_new(connId, cwd)`,
  `acp_session_load(connId, acpSessionId, cwd)`,
  `acp_prompt(connId, sessionId, text)`, `acp_cancel(connId, sessionId)`,
  `acp_session_close(connId, sessionId)`,
  `acp_set_config(connId, sessionId, configId, value)`,
  `acp_respond_permission(connId, requestId, optionId?)`.

### Per-session invariants (verify on every ACP change)

These are the contracts that make «one process per session» correct. Each was
validated in review; a regression in any of them silently breaks multi-session
isolation.

1. **Route strictly by `connId`.** Every event handler (`applyUpdate`,
   `onStatus`, permission queue) resets/targets state via `sessionStore.get(connId)`.
   Never map an agent-side `sessionId` back to a local session — that reverse
   lookup (`findByAcpSessionId`) was deleted on purpose.
2. **Per-session tool-call host key.** `toolCallHosts` is keyed by the composite
   `connId:toolCallId` — two sessions' agents may emit the *same* `toolCallId`,
   and a bare-id key would patch the wrong session's message.
3. **`closeConnection` = kill that process.** Generating → `session/cancel` first,
   then `acp_disconnect(sessionId)` (close stdin → ≤2s → kill → remove → emit
   `acp://status {connId, stopped}`). Closing a tab / deleting a session must
   reach this path.
4. **`dead` is scoped.** A reader-loop EOF clears only that conn and emits
   `acp://status {connId, dead}`; every other session stays connected. `mark_dead`
   must never clear the whole registry.
5. **Health chip is per-session.** `connecting`/`connected` are derived from the
   local session id's membership in the live set (`liveAcpSessions`), plus a
   per-session connecting mirror (`acpConnectingSessionIds`). No global
   `acpConnecting` bool may drive a chip.
6. **Concurrent connect dedup.** Rust holds a per-session start lock and re-checks
   `CONNS` under it; the frontend shares an in-flight promise per session
   (`connectInFlight`). Two rapid connects must spawn exactly one process.
7. **`StatusResult` carries `connId`.** `acp_status` returns `connId` so the TS
   type (`AcpStatusEvent`) is honest; keep Rust and TS field-for-field.
8. **Idle sweep.** 180s default (env `LIGHTR_ACP_IDLE_TIMEOUT_SECS`, `0` = off);
   only disconnects when connected AND not generating AND no pending
   request/permission. Activity (prompt / update / permission) refreshes the timer.

### Process spawn (`build_acp_command`)

Every connection (`connect_inner`) and the one-shot probe (`probe_options_inner`)
build the child through **one** helper, `build_acp_command()` (`resolve_spawn()` →
real exe or `cmd /C opencode acp`, piped stdio, `CREATE_NO_WINDOW` on Windows). It
**removes `OPENCODE_SERVER_USERNAME` / `OPENCODE_SERVER_PASSWORD`** from the child env.

> **Gotcha**: those two vars are exported by *another* running opencode instance
> and leak into our spawned `opencode acp`. With them present, opencode's free
> Zen models fail upstream with `acp:-32603 ... Invalid credential`. Removing
> them restores the clean, credential-less state codeg gets by launching
> `opencode acp` independently. Only these two are stripped; `OPENCODE`,
> `OPENCODE_PID` and user-provider keys must pass through.

### One-shot model probe (`acp_probe_options`)

`acp_probe_options(cwd?)` spawns a **throwaway** `opencode acp`, runs
`initialize` → `session/new`, returns `configOptions`, then closes stdin and
reaps the child. It never touches the `CONNS` registry, never emits `acp://*`,
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

## Agent / engine registry (`src-tauri/src/agent_type.rs` + `engine.rs`, task 10-13-acp-multi-engine, extended by 10-13-acp-claude-codex)

The ACP layer is **engine-pluggable**, modeled on codeg's multi-agent abstraction but
trimmed to "spawn a system-installed engine" (no download/install/version-pinning/
custom agents/DB — those are out of scope). Two Rust modules:

- **`agent_type.rs`** — `AgentType` compile-time enum (`OpenCode`, `ClaudeCode`, `Codex`)
  with `as_wire()` / `from_wire()` / `Display` / `Serialize` / `Deserialize` and
  `BUILTIN_AGENT_TYPES`.
- **`engine.rs`** — `AcpAgentMeta { agent_type, name, description, launch: LaunchSpec }`,
  `get_agent_meta(AgentType) -> AcpAgentMeta` (exhaustive `match`), `AcpAgentInfo`
  (camelCase serde), `list_agents()`, and `bare_bin_name()`.

### Wire contract (persistence boundary — DO NOT rename)

`AgentType::as_wire()` is the **stable persisted form** written to
`SessionIndexEntry.engineId` / `Session.engineId`. Current values: `"open_code"`,
`"claude_code"`, `"codex"` (the latter two mirror codeg `models/agent.rs` for future
data interop). Renaming a wire string silently orphans every saved session that
references it — the `builtin_wire_names_are_unchanged` test locks the mapping (mirrors
codeg). Unknown wire → `from_wire` returns `None`; Rust commands error, frontend falls
back to `DEFAULT_ENGINE_WIRE` (`'open_code'`).

### Two launch forms: `DirectCommand` vs `ExternalAdapter` (task 10-13-acp-claude-codex)

`LaunchSpec.kind: LaunchKind` selects how the engine process is resolved:

- **`DirectCommand`** (OpenCode) — the engine IS the vendor CLI with an ACP subcommand
  (`opencode acp`). Resolved via `resolve_bin` = PATH scan of `bin_candidates` only.
- **`ExternalAdapter`** (Claude Code, Codex) — the vendor CLI does **not** speak ACP;
  a separately-installed ACP adapter bridges it (`claude-agent-acp` / `codex-acp`,
  published by the Agent Client Protocol org). The adapter command may live in **PATH
  or the npm global bin dir** (`npm prefix -g` → `<prefix>` on Windows / `<prefix>/bin`
  on Unix). Resolution order: PATH → npm global bin → `SpawnSpec::NotInstalled{hint}`.

**User-installs, we never bundle or download** (AC11). The adapters are heavy
(`claude-agent-acp` ~287MB incl. the Anthropic SDK; `codex-acp` pulls `@openai/codex`
whose `codex.exe` is ~326MB), so packaging them is not viable. `LaunchSpec.install_hint`
carries the npm package name **only to render the `npm i -g <hint>` guidance** in the
`NotInstalled` error and detect `detail` — it never triggers a download. There is no
`npm install` / `npx` spawn anywhere in `src-tauri/src/`; the only npm invocation is the
local query `npm prefix -g` (`resolve_npm_global_bin_dir`).

`SpawnSpec` is the single generic spawn shape (no engine names):

```rust
pub enum SpawnSpec {
    Exe(PathBuf),                        // resolved real executable
    CmdFallback,                         // DirectCommand: `cmd /C <bare> [subcommand]`
    NotInstalled { hint: String },       // ExternalAdapter, nothing resolved → Err, no spawn
}
```

`build_acp_command(meta) -> Result<Command, String>` (returns `Result` so `NotInstalled`
propagates before any `spawn()`); both call sites (`connect_inner`, `probe_options_inner`)
use `?`. `command_from_spawn_spec()` is the pure, unit-tested argv builder.

> **Warning (codex optional-dep gotcha)**: `@openai/codex`'s platform binary is an npm
> **optional dependency** (`@openai/codex-win32-x64` → `@openai/codex@<ver>-win32-x64`).
> Some npm configs skip it, leaving a missing/truncated `codex.exe` → adapter dies with
> `Missing optional dependency ...` / `spawn EFTYPE`. This is a **user environment**
> issue; the fix is `npm i -g @openai/codex@latest`. Our code surfaces a readable hint
> but does not repair it.

> **Warning**: `agent.rs::run_engine` must run the **resolved** bin (`resolve_bin_for_detect`
> result), not `bin_candidates[0]` verbatim — an `ExternalAdapter` installed only in the
> npm global bin dir is invisible to PATH, so the `--version` probe would spuriously fail
> even though `installed = true`. `run_engine(meta, resolved_bin, args)` takes the resolved
> path and falls back to the bare name (opencode behavior preserved).

### Command surface

- `acp_connect(sessionId, engineId)` — engineId is the wire string; parsed to
  `AgentType` and bound to `AcpConn.engine`. Session-level commands
  (`session_new/load/prompt/cancel/set_config/respond_permission/session_close`)
  remain **connId-only** — engine is read from the connection.
- `acp_probe_options(cwd?, engineId?)`, `agent_model_context_window(provider, model, engineId?)`
  — engineId optional, defaults to opencode.
- `agent_detect(engineId)`, `agent_read_config(engineId)`, `agent_write_config(engineId)`
  — the old `agent_*_opencode` names are retired (single rename, no alias).
- `acp_list_agents()` → `AcpAgentInfo[]`; `engine_default_get()` / `engine_default_set(engineId)`.
- `agent_read_env_file` / `agent_write_env_file` stay engine-agnostic (app-level env).

### `agent_model_context_window` signature

`(provider, model, engineId?)`. **`provider` and `model` are separate params** (not a
joined `provider/model`). The Rust side dispatches on the engine's
`ContextWindowSource` (`OpenCodeConfigAndCatalog` today) — the opencode
config→models.dev-cache lookup lives behind that variant, not inline in a branch.

### Engine-specific code placement (AC8)

Protocol routing (`reader_loop` / `handle_incoming` / `handle_client_request`) **must
not branch on engine name**. Engine-specific code lives only in: spawn construction
(`resolve_bin`/`resolve_spawn`/`build_acp_command` take `&AcpAgentMeta`, and dispatch
only on `LaunchKind`/`SpawnSpec`, never on `AgentType`), detect chain (`agent.rs`,
engine-ified end to end), config path, and context-window lookup. All constants (bin
candidates, subcommand, env strip, detect args, config path, install hint) come from
`LaunchSpec`. Adding an engine = new enum variant + one `match` arm in `get_agent_meta`.
Verify with `rg -n "claude|codex|ClaudeCode|Codex" src-tauri/src/acp.rs` — every hit must
be inside `#[cfg(test)]`.

> **Warning**: `env_strip` is engine data now (opencode strips
> `OPENCODE_SERVER_USERNAME`/`OPENCODE_SERVER_PASSWORD`; claude/codex are `&[]`). New
> engines declare their own.
> The Windows fallback uses `bare_bin_name()` (extension-stripped) so `cmd /C <bin>`
> keeps PATHEXT resolution — never feed it `bin_candidates[0]` verbatim.
> Windows `bin_candidates` for `ExternalAdapter` must lead with the `.cmd` shim
> (`claude-agent-acp.cmd`, `codex-acp.cmd`) — npm installs `.cmd`/`.ps1`, not a bare name
> or `.exe`.

### Adapter detection & fields (task 10-13-acp-claude-codex)

- `detect_args = ["--version"]`, `acp_probe_args = ["--version"]` for both adapters.
  Measured: `claude-agent-acp --version` → `0.89.1` (exit 0); `codex-acp --version` →
  `@agentclientprotocol/codex-acp 2.2.2` (exit 0). `--help` is **unreliable**
  (`claude-agent-acp --help` exits 0 with no output; `codex-acp --help` crashes with
  `0xC0000409`), so it is never used as a probe.
- `config_path = None` for claude/codex (no independently editable config; credentials
  are managed by the vendor CLI in `~/.claude` / `~/.codex`). `agent_read/write_config`
  therefore returns empty / errors for them and the settings page shows a generic
  placeholder instead of a config editor.
- `context_window = ContextWindowSource::None` (returns `Ok(None)`; the frontend keeps
  its existing denominator). The first `session/new` measured claude ~5.4s / codex ~1.7s,
  within the existing 10s/30s budgets — no timeout change needed.
- The adapters **bundle their own vendor CLI** (`@anthropic-ai/claude-agent-sdk-*/claude.exe`,
  `@openai/codex/vendor/.../codex.exe`), so the vendor CLI need not be on PATH; auth
  problems surface at prompt time as normal ACP auth errors.
- `session/new`'s `configOptions` is a **standard array** for both (claude 3 items,
  codex 5) — the existing model/mode dropdown consumption path is reused unchanged.

### Frontend

- `src/types/engine.ts` — `EngineWire` (`'open_code' | 'claude_code' | 'codex'`),
  `AcpAgentInfo`, `BUILTIN_ENGINE_WIRES`, `DEFAULT_ENGINE_WIRE`.
- `src/services/engineService.ts` — sole `invoke` site for `acp_list_agents` /
  `engine_default_get` / `engine_default_set`; isTauri dual-channel + mock.
- `src/stores/engine.ts` — **single shared store** (like codeg `use-acp-agents`):
  `agents` (sorted `sortOrder` + name), `fresh`, `defaultEngineId`,
  `preselectedEngineId` (default → first-available fallback, **without writing back
  the default** — fallback ≠ user confirmation), `load()`/`refresh()`,
  `setDefaultEngine()`, `pickEngine()`, `agentName()`. On fetch failure it keeps the
  last list (never clears); browser dev falls back to a static 3-engine entry.
- `NewSessionDialog.vue` — engine picker at new-session entry points
  (`SidebarPanel.vue` folder/tmp, `CodingWorkspace.vue`, `TopBar.vue`). Confirm writes
  `engineId` into the created `Session`. Supports `engineOnly` mode (folder already
  fixed by the caller — the picker hides the folder select and shows a read-only target);
  the sidebar folder-row 「新建会话」 uses it. **Engine is chosen here (at creation) and
  nowhere else**: once a session exists its `engineId` is frozen (R8) — there is
  deliberately NO engine switcher in the composer.
- Engine **icon** mapping is single-sourced in `src/utils/engineIcons.ts`
  (`engineIcon(wire)` → lobehub `?raw` SVG). Prefer `*-color.svg` variants (brand color
  baked in); mono variants use `fill=currentColor`, so the container must set an
  explicit text color or it renders black.
- After the dialog confirms and the draft is created, `SidebarPanel` calls
  `ensureModelOptions(sessionId)` to pre-warm that engine's model list (probe keyed by
  engine; failure silent — the composer still loads it asynchronously).
- `AgentManageView.vue` — list from the engine store, reactive selection, plus a
  "default engine" chooser writing `settings.json`.

### Per-engine status isolation (`stores/agent.ts`, closed in 10-13-acp-claude-codex)

**Design rule**: `detectResults` is bucketed **by engine wire**, and every list row reads
its own bucket. Never hand a shared/global status value to every row.

`stores/agent.ts` exposes per-engine projections for this:

- `detectingEngineId` (`ref<string|null>`) — the engine currently being detected, set by
  `detect()` / updated per id inside `detectAll()`, cleared in `finally`.
- `agentStatusFor(engineId)` — `'detecting'` **only** when
  `detectingEngineId === engineId` (a global `detecting` bool would light up every row's
  spinner), else maps `detectResults[engineId]` → `'unknown' | 'not-installed' | 'pass' | 'fail'`.
  The legacy single-value `agentStatus` computed was **removed** (all callers now pass an
  explicit engine id to `agentStatusFor`); do not reintroduce a global status for rows.
- `runDetect(target)` (pure: writes only `detectResults[target]`) vs
  `loadSupporting(target)` (config + app-level env side effects) — split so
  `detectAll()` can sweep several engines and then load the shared config/env cache
  **exactly once** for `activeEngineId` (a per-engine loop that called `loadSupporting`
  for each id would let the last engine's `configExists:false` clear opencode's loaded
  config from under the detail panel).

Row/action wiring in `AgentManageView.vue`:

```
onMounted   → engineStore.load() → set activeEngineId (= selectedAgentId) → detectAll(all agentTypes)
onSelect    → selectedAgentId = id → watcher writes activeEngineId → detect(id)
row @refresh→ refresh(rowId) → detect(rowId)   // NOT the currently selected engine
watch(selectedAgentId) → if builtin wire: agentStore.activeEngineId = id
```

**Concurrency contract**: `detect()` / `detectAll()` share the `detecting` re-entrancy
lock (a second caller returns early). Both clear `detecting`/`detectingEngineId` in
`finally`, so a thrown service error cannot leave a row spinning. Callers in the view
must handle rejection explicitly (`refresh()` catch → `message.error`;
`onMounted` catch → `message.error`) — an un-awaited `onMounted` promise rejection is
an unhandled rejection, not a silent no-op.

**Browser mock parity**: `mockDetect(engineId)` mirrors the real contract —
claude/codex (external adapters the user must `npm i -g`) return
`installed:false` + `detail`, only opencode passes. `engineService.mockList()`
must keep `available` consistent with that (claude/codex `available:false`,
`installedVersion: null`), otherwise the new-session dialog shows a green engine the
settings page calls 未安装.

### Known gap (second-engine work)

**Closed** (10-13-acp-claude-codex): `stores/agent.ts` used to keep `detectResults`
keyed by engine wire while nothing wrote `activeEngineId`, so
`AgentManageView.refresh()` wrote `detectResults[selectedAgentId]` and the detail panel
read a different key. Now the `selectedAgentId → activeEngineId` watcher (plus the
explicit write in `onMounted`) keeps them in sync, and rows call `agentStatusFor(id)`.

Remaining, documented edge cases (not bugs, but keep them in mind when adding engines):

- `prechecks` / `detectResult` / the config cache are still **single-slot**, keyed by
  `activeEngineId`. Only one engine's detail can be shown at a time; `detectAll()`
  deliberately loads the cache for `activeEngineId` only.
- `useConversation.passGate()` caches detect per session engine in the same
  `detectResults` map; a session whose engine was never detected triggers `detect()`.
  It ignores the store's re-entrancy early-return, so a concurrent detect can leave the
  gate evaluating a stale/absent result. Values resolve after the in-flight detect
  completes — acceptable today, revisit if detect becomes slow for a third engine.

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
