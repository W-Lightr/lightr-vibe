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
  (`method` + `id`) → `request_permission` + `elicitation/create` forwarded to the UI,
  `fs/*` / `terminal/*` (and any other `elicitation/*` sub-method) refused with `-32601`.
- **JSON-RPC ids are preserved as raw `Value`** — agent may legitimately use
  string ids. Never model an incoming id as `u64`, and never treat `id: null`
  as a response.
- Events **carry `connId`** (local session id) so the frontend routes per connection:
  `acp://update` `{connId, sessionId, update}` · `acp://permission`
  `{connId, requestId, sessionId, toolCall, options, meta}` ·
  `acp://question` `{connId, requestId, sessionId, questions}` ·
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
`src/utils/permissionMeta.ts` (`parsePermissionTitle` / `toolDisplayName` /
`permissionTarget` / `parsePermissionDescription` / `isDefaultToNo` /
`parsePermissionCommand` / `parseOptionChanges` / `extractDiff` / `optionLabel`).
`PermissionCard.vue` must import these, never re-parse `_meta` inline (code-reuse
rule). Parsers are **strict**: `_meta.permission` is honoured only when
`version === 1`; `defaultToNo` counts only for the literal `true`; anything
unrecognised falls back to `toolCall.title` / the JSON summary / the legacy three
buttons (allow once / always / reject) — so an opencode request with no `_meta`
renders exactly as before (zero regression).

**Card UI shape** (task 10-14-acp-permission-rich, revised): the card is a
**compact standalone box** rendered inside `MessageComposer.vue`, floating
**above** the composer input — its own `rounded-2xl border`, horizontally
**narrower** than the input box (`mx-3 sm:mx-6`), with a small gap (`mb-1.5`); it
does **not** merge into the composer border. Layout: title = **Chinese tool name**
(`toolDisplayName`, e.g. `edit`→编辑文件) + one-line target (`permissionTarget`,
basename/command) + right-aligned buttons. Command / diff / `_meta` grants / raw
input are **collapsed** behind a「详情」toggle (not shown inline). `optionLabel`
prefers the **Chinese** `kind` label over the agent's English `name`
(opencode sends `Allow once` / `Always allow` / `Reject`).

**Queue count is a frontend projection**: `useAcpPermission` queues *every* pending
request per connection; `MessageComposer` filters by `connId` and renders the
list, so the card's「还有 N 个待审批」is `permissionQueue.length - 1 - i` — the Rust
`pending_perm` map is intentionally **not** turned into a server-side queue (only
needed for codeg's single-slot snapshot, which we do not have).

> **Gotcha (cost a real hang)**: filtering the pending queue **must use `connId`**
> (`e.connId === session.id`), never `e.sessionId`. `sessionId` is the agent-side
> id (`ses_xxx`) and is **never equal** to the local `session.id` (`ses-xxxxxxxx`),
> so a `sessionId`-keyed filter matches nothing → the card never renders → the
> engine blocks on `session/request_permission` forever (UI shows「正在生成…」with
> no card). This is the event-routing invariant applied to the permission queue;
> the mock hides it only if its `sessionNew` happens to return the local id.

### AI questions / `elicitation/create` (task 10-14-acp-elicitation)

`elicitation/create` (server→client) is how an agent asks the user a question
(codex Plan-mode `request_user_input`, opencode agent questions, generic MCP
forms). Previously refused with `-32601` → those questions **silently failed**.
Now handled for the **Questions** shape only:

- **Rust** (`acp_question.rs`): pure `classify_elicitation(&Value) -> ElicitationPlan`
  reads `params.mode.requestedSchema.properties` (lenient) → `Questions(Vec<Question>)`
  or `Decline`. Field mapping: `type:"array"` → multi; `type:"string"` → single if
  it carries options, else free text; any other type / empty/missing schema →
  `Decline`. **Option source is dual-shape**: `enum` (+ optional `enumNames`) OR
  `oneOf`/`anyOf` whose elements are `{const,title}` — the latter is **opencode's**
  shape (single: `type:"string"` + `oneOf`; multi: `type:"array"` + `items.oneOf`).
  Missing the `oneOf` form would silently mis-read an opencode single-select as free
  text. **One question at a time** (first field; the rest are dropped — documented
  limitation; opencode's `<key>_custom` companion fields are ignored).
  `build_elicitation_result` maps answers → `{action:"accept", content:{id:
  string|string[]}}`, or `{action:"decline"}`.
- **Rust** (`acp.rs`): `AcpConn.pending_question` is a **parallel** map to
  `pending_perm` (do NOT overload the permission map). `handle_client_request`
  intercepts `elicitation/create` **before** the `fs/|terminal/|elicitation/`
  `-32601` block: Questions → store + emit `acp://question`; Decline → reply
  `{action:"decline"}` directly (agent continues, never hang). `acp_answer_elicitation`
  writes the JSON-RPC response and removes the entry (missing id → safe `Ok(())`).
  `cancel_by_rpc_id`, `acp_cancel`, `disconnect_conn`, `mark_dead`, and the idle
  busy-check all drain `pending_question` (decline — the protocol's MUST-respond
  obligation; a dead pipe is the one case where writing is impossible).
- **Capability**: `client_capabilities()` **unconditionally** advertises
  `elicitation.form` for every engine (D2 revised). Evidence: opencode's provider ACP
  docs (CLI/ACP §"Questions") state it sends `elicitation/create` in form mode *when
  the client advertises `elicitation.form`*; `claude-agent-acp` ≥0.43.0 adds
  experimental support; codex always supported it. An engine that doesn't support it
  simply never sends — declaration is a hint, not a coercion. The function takes **no
  engine argument** → there is no engine-name branch anywhere in routing OR
  declaration (AC9 stronger than before). Declined/unsupported shapes still fall to
  `Decline` (never hang).
- **Frontend**: `useAcpQuestion.ts` mirrors `useAcpPermission` (module singleton,
  queue filtered by **`connId`** — same invariant warning as the permission queue),
  `AskQuestionCard.vue` renders single/multi/text + 提交/跳过, mounted above the
  composer; a local 「AI 提问 / 我的回答」 text message is pushed back into the
  transcript. Service owner: `acpService.answerElicitation(...)` (sole invoke site).
- **Out of scope now**: Approval-style elicitation (MCP tool approval / message-only
  confirm), boolean/number/URL fields, multi-question forms, `ask_user_question`
  user toggle.

### Injected ask tool (`lightr-ask-mcp`, task 10-15-acp-ask-tool)

Engines that don't speak ACP `elicitation/create` (e.g. opencode **1.x** — the
"Questions" feature is ACP-v2-only; without a tool the model improvises a native
PowerShell `Show-MessageBox`) get a **self-owned ask tool** injected, mirroring
codeg's `codeg-mcp` companion but simplified:

- **Companion bin** `src-tauri/src/bin/lightr_ask_mcp.rs` (`[[bin]] lightr-ask-mcp`,
  std-only, does NOT link Tauri): a minimal newline-delimited JSON-RPC **stdio MCP
  server** exposing one tool, `ask_user_question` (inputSchema: `question` (required),
  `options[]`, `multiSelect`, `header`). On `tools/call` it connects to `--socket`
  with `--token`/`--conn-id`, sends the raw arguments, **blocks** for the answer, and
  returns it as MCP `content` (declined → a "skipped" text result).
- **Main-process bridge** (in `acp.rs`): `ASK_LISTENER` (a `OnceLock` bound once to
  loopback TCP `127.0.0.1:0`), per-connId token table (`ask_token_for`/register/
  unregister), `handle_ask_connection` (validate token → `questions_from_mcp_args` →
  store pending → emit → block), `write_ask_response`, `render_answer_text`.
  **Only loopback + per-session token**; never a network port.
- **Injection**: `mcp_servers_for(conn_id)` returns the ACP `McpServerStdio` entry
  `{type:"stdio", name:"lightr-ask", command:<sibling lightr-ask-mcp exe>,
  args:[--socket,127.0.0.1:<port>,--token,<t>,--conn-id,<connId>], env:[]}` and is
  passed as `session/new` / `session/load` `mcpServers` (was `[]`). **Returns `[]`
  when the companion exe isn't found** (→ identical to before; opencode zero-regress).
  The throwaway probe does NOT inject. The injectable decision is gated only on
  bin-found + listener-present — **no engine-name branch** (AC6/PAC1).
- **Two reply backends, one card**: `pending_question` value is
  `(QuestionReply, Vec<Question>)` where `QuestionReply::Acp(rpcId)` writes a JSON-RPC
  response (native elicitation) and `QuestionReply::Mcp(Sender<AskOutcome>)` pushes
  the answer over the socket. `acp_answer_elicitation` / `decline_elicitation_inner`
  branch on it. Frontend is **unchanged** — the MCP ask reuses the exact same
  `acp://question` event (carries `connId`) + `AskQuestionCard` + `useAcpQuestion`.
- **Never hang**: cancel/disconnect drains `pending_question` → `decline` (Mcp arm
  sends `declined` → socket unblocks); `mark_dead` clears the map → drops the Sender
  → the socket thread's `recv` errors → writes declined. `cancel_by_rpc_id` matches
  only `Acp` (an MCP ask has no rpc id — it's canceled via socket/session close).

> **Warning**: `default-run = "lightr-vibe"` is required in `Cargo.toml` now that the
> package has two bins — otherwise `cargo run` (Tauri dev) fails with "could not
> determine which binary to run".

### `session/update` kind consumption (task 10-14-acp-update-kinds)

`reader_loop` forwards the whole `session/update`, so the frontend can consume any
kind. `useConversation.applyUpdate` now handles (beyond text/tool/usage/config):
`plan` · `session_info_update` · `available_commands_update` · `current_mode_update`.

- **`plan`** → `sessionStore.setPlan(connId, entries)` (overwrite; empty clears);
  `PlanCard.vue` renders items + status badges. Entries `{content, priority?, status?}`
  (status `pending|in_progress|completed`).
- **`session_info_update`** → `title` updates the session title via `renameSession`
  (native title **overwrites** a user rename — documented concession); retry state via
  the **single owner** `src/utils/sessionUpdateMeta.ts` `parseRetryState(meta)` /
  `hasRetryMeta(meta)` → `sessionStore.setRetry`, shown as a **retry banner** (amber,
  above the composer) — NOT a failure card.
- **`available_commands_update`** → `setCommands`; **`current_mode_update`** →
  `setCurrentMode` (`currentModeId` + mirrors `mode`).
- **`acp://configRejected`** (new event) — `ConfigOptionRejected` is **not a wire
  kind**: `acp.rs::acp_set_config` compares the agent's adopted `currentValue` against
  the requested value (`detect_config_rejection`, pure) and emits
  `acp://configRejected {connId, configId, requested, actual}` on mismatch → the
  frontend rolls back the optimistic value + toasts. `acp://update` also carries a
  top-level `meta` (from `params._meta`) alongside `update`; retry `_meta` is read as
  `update._meta ?? event.meta` (lenient — engines differ on where `_meta` sits;
  opencode's retry/compaction `_meta` is v2-only, absent → harmless no-op).

> **Gotcha**: retry must be cleared **only** when a retry-carrying update arrives
> (`hasRetryMeta`) or at the turn boundary (one `finally` covering success/error/cancel).
> Clearing on *any* `session_info_update` wrongly hides a live banner when a
> title/compaction-only update arrives mid-retry.

Unknown / unhandled kinds still fall through to the default branch (safe no-op);
AIR-heavy kinds (steering/goal/sessionFailure/contextCompaction/fork/delegation/
background) remain out of scope.

### `notice` — ACP 1.9 Session Notices (task 10-14-acp-stream-control, AC10 real-machine fix)

`claude-agent-acp` on a gateway (nexus365.cc) rendered a long `**Warning:** We're
changing auto mode…` block **as assistant chat text** — a capability gap, not a stream
bug. With `initialize.clientCapabilities.session.notices` advertised, claude sends the
warning as its own wire kind instead of degrading into `agent_message_chunk`.

- **Capability** (`acp.rs::client_capabilities()`): emits
  `{ "session": { "notices": {} } }` **unconditionally** (no engine branch, PAC1).
  `notices` must be an **object** (adapters test `typeof session.notices === "object"`).
  Accepted tradeoff (same as codeg): claude stops sending `level === "info"` frames.
- **`session.compaction` is deliberately NOT advertised.** codeg advertises both, but its
  compaction consumer maps `compaction_update` back into `_meta.contextCompaction`; we
  have **no** compaction consumer, so advertising it makes claude stop sending the
  existing synthetic compaction tool call → pure regression. Rust test
  `client_capabilities_advertises_session_notices_only` pins both facts.
- **Wire shape**: `{ sessionUpdate: 'notice', severity?, title?, description? }`; Rust
  needs **no routing change** (`reader_loop` forwards `params.update` verbatim).
- **Frontend**: `src/utils/sessionNotice.ts` is the **single owner** of decoding
  (`noticeLevel` severity→level, unknown ⇒ `info`; `splitHeadline` first non-empty line;
  `noticeToast` returns `null` for a title-less notice — codeg drops those). `applyUpdate`
  `case 'notice'` writes `sessionNoticeState` (seq-based module slot, same pattern as
  `configRejectedNotice`) and **must not** touch the transcript. `MessageComposer.vue`
  watches it (filtered by `connId`) and calls `message[level](text, {...})` — the existing
  naive-ui toast, no new toast system.
- Mock: `acpMock` emits one `warning` notice when the input contains 「通知」.

### Stream control — stop / empty-reply / reconnect (task 10-14-acp-stream-control)

The turn lifecycle is driven by the `acp_prompt` promise's `stopReason`. Frontend
contract (`useConversation.ts`, `acpMock.ts`, `src/types/session.ts`):

- **Empty-reply failure inference (D2, PRD R1).** A `prompt` that resolves with
  `stopReason === 'end_turn'` **and** whose turn has *zero content* is a silent
  failure, not success → push an assistant card
  `{type:'error', errorKind:'empty', content:'Agent 未返回任何内容（可能已失败），请重试'}`.
  - `turnHasContent(sessionId, turnId)` is the single owner of "has content".
    Content = any **assistant** message on that `turnId` with non-empty `content`
    (text or thinking), `toolCalls.length`, or `type:'error'`. `user` messages
    never count (the sender's own bubble would always make a turn look non-empty).
  - Trigger is **only** `end_turn`: `cancelled` stays `stopped`, `refusal` already
    has its own card — neither is judged empty.
  - Judgement is **conservative**: any tool call or thinking content makes the turn
    non-empty (a pure-tool or pure-thinking turn is never flagged).
  - Defensive drain: before pushing the empty card, any residual `status:'streaming'`
    message on that `turnId` is patched to `status:'error'` so no empty `done`
    bubble coexists with the failure card (normal empty replies have none).
- **`stop()` semantics** (three things): `session/cancel` + `cancelQueued`
  (permissions) + `cancelQueuedQuestions` (elicitation). It converges the current
  turn; a late `end_turn` must **not** flip an already-`stopped` message back to
  `done` (`finalStatus` applies only while `status === 'streaming'`).
- **Retry ≠ failure.** While a retry is in flight (`hasRetryMeta`/`parseRetryState`)
  the amber **retry banner** shows and the empty/error failure card must **not**
  also appear — a prompt still pending is not a failure. The two are mutually
  exclusive.
- **Thinking-only turns keep their actions** (AC10 fix). A turn that produced only a
  `thinking` message renders in the thinking branch of `MessageItem.vue`, which is
  separate from the text branch. Both branches must expose the action toolbar
  (重新生成 / 从此分叉) once the turn converges (`!isStreaming`) — otherwise a
  thinking-only turn shows no 重新生成 button. The two branches are mutually
  exclusive (`v-else-if` / `v-else`), so there is never a duplicate toolbar.
- **Thinking and text chunks may share one `messageId`** (AC10 fix, claude-agent-acp).
  The adapter emits `agent_thought_chunk` **and** `agent_message_chunk` with the
  **same** `messageId` (one `chatcmpl-…` for the whole turn). Keying the local message
  by `messageId` alone therefore merges the answer into the thinking bubble — the
  symptom is「只有思考内容、没有正文」. The local id must carry the kind:
  `` `${messageId}:thinking` `` / `` `${messageId}:text` ``, and the lookup must also
  match `type`. Never key assistant chunk messages by `messageId` alone.
- **`reconnect()` first converges a generating session.** If the local session is
  generating, `reconnect()` calls `stop(sessionId)` **before** `disconnect()`;
  reconnect kills the old process, so an in-flight `prompt` would otherwise never
  resolve (its turn would hang forever).
- **Every failure card that says「请重试」must offer the retry button**
  (`MessageItem.vue::showRetry`). The error branch is `v-else-if="isError"` and is
  mutually exclusive with the text/thinking branches that own the action toolbar, so
  a card promising a retry with no entry point is a dead end. `regenerate()` looks the
  user message up by `turnId` and works for any assistant-side card that carries one
  (empty / generic / refusal), which is why `showRetry` gates on `turnId` +
  `errorKind ∈ {empty, generic}` — `dead`/`auth` need the environment fixed first
  (「去设置」only) and `timeout` keeps the user's decision.
- **Turn-boundary retry clearing must be a real state change.** `setRetry(sessionId,
  null)` assigns `undefined`; assigning the *same* `undefined` over `undefined` makes
  Vue's reactive setter bail out (`hasChanged` false) and no dependent recomputes — so
  a banner whose last update touched nothing else in the session would stay visible.
  Write only when `get(sessionId)?.retryState !== undefined`, so the `null → undefined`
  write is the change that clears it.

> **Gotcha**: `stop()` only cancels the current turn. Do **not** reuse it to mean
> "tear down the connection" — that is `closeConnection()` (see per-session
> invariants, item 3). Reconnecting while generating requires *both*, in the order
> `stop()` → `disconnect()`.

Other ACP behaviors worth keeping:

- `tool_call_update.content` is **cumulative replacement** — overwrite, never append.
- `session/load` replays history as `session/update` before responding; suppress
  those updates for that session or the local transcript doubles.
- `session/cancel` is a notification (no response awaited); idle cancel is a
  safe no-op; cancel must also resolve that session's pending permissions.
- Permission option selection must match by `kinds` (`allow_once` before
  `allow_always`), never by array position.
- Timeouts: initialize 60s / `session/new` 30s / `prompt` 300s / other 30s.
  The first `session/new` on a cold agent takes 8–13s (model enumeration).
  Initialize is 60s (raised 10s→60s, AC10 real-machine feedback, matching codeg's
  `connection.rs` 60s): claude-agent-acp preheats Claude Code's MCP servers before
  answering initialize, so a tight bound surfaced as `timeout:10s initialize` on
  reconnect. The adapter itself answers in 0.6–2.2s.
- **node-shim spawn trap (`engine.rs::extract_exe_from_shim`).** npm-installed
  adapters (`claude-agent-acp`, `codex-acp`) ship a `.cmd` shim whose real command
  is `node <…\dist\index.js>` — the exe token is a bare `node.exe` interpreter and
  the script is a **separate argument**. Extracting only the `.exe` spawns bare
  `node`, which reads the JSON-RPC line as JavaScript and throws a SyntaxError →
  initialize never answers (symptom: `timeout:… initialize`, intermittent by which
  shim resolves). The extractor must **reject a node/node.exe interpreter** and
  return `None` → `CmdFallback` (`cmd /C <bare_bin>`), which runs the shim correctly
  (~1.8s handshake). Only a single self-contained `.exe` target is extractable.
- Always `create_dir_all(cwd)` before `session/new` — the protocol requires an
  existing absolute cwd.
- Command surface (all take `connId`, camelCase params): `acp_connect`,
  `acp_disconnect`, `acp_status`, `acp_session_new(connId, cwd)`,
  `acp_session_load(connId, acpSessionId, cwd)`,
  `acp_prompt(connId, sessionId, text)`, `acp_cancel(connId, sessionId)`,
  `acp_session_close(connId, sessionId)`,
  `acp_set_config(connId, sessionId, configId, value)`,
  `acp_respond_permission(connId, requestId, optionId?)`,
  `acp_answer_elicitation(connId, requestId, answers, declined)`.

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

**Engine-agnostic fallback (task 10-14-acp-stream-control, AC10 fix).** Claude Code
and Codex declare `ContextWindowSource::None` (no config/catalog lookup), so the
above opencode sources don't apply. For them the resolver falls back to
`infer_context_window_from_model(model)` (ported from codeg
`parsers::infer_context_window_max_tokens`), then to a UI **hint**:

1. **Capacity suffix** — `[1M]` / `[500k]` / `[1.5m]` in the model id; authoritative,
   works for gateway names like `deepseek-v4.1-flash[1M]`.
2. **Claude family heuristic** — `claude*` with a `\b1m\b` id token OR a natively-1M
   family (all fable/mythos; opus ≥ 4.7; sonnet ≥ 5; haiku ≥ 5.5) → 1_000_000, else
   200_000.
3. **UI hint text** — `agent_model_context_window(provider, model, engineId?, hint?)`
   takes an optional `hint` = the selected model option's `name` + `description`
   (`"deepseek-v4.1-flash Custom Sonnet model (1M context)"`). `parse_context_hint_text`
   sniffs `1M`/`500k`/`1,000,000`/bare ≥100000 out of it. This is why the frontend
   passes the option label: Claude's model option **value** is a bare name (`fable`,
   `sonnet`) with no provider prefix and no suffix — the capacity lives only in the
   label/description. `provider` may be empty (the opencode branch skips).

The composer resolves this **on session open and on `[modelOption.id, model]` change**
(`resolveContextWindow`), not only on manual model switch, so the denominator isn't
stranded at the 128k placeholder. `resolveContextWindow` bails when `session.model` is
unset — never fall back to `workspace.selectedModel` (another engine's default).

**Monotonic denominator.** `sessionStore.setContextUsage` takes the **max** of the
current and incoming `usage_update.size` (only grows). Claude/Codex report an
unstable size within one session (observed 800000 → 1000000), and a smaller value
would flicker/regress the gauge; capacity is a session-level constant, so growing it
is always right and shrinking is adapter noise.

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

## Task commands (`src-tauri/src/task.rs`, task 10-17-task-data-layer)

The work-task board's data layer: one JSON document under the app namespace, mutated
only through `task_*` commands. This is the **only** place `tasks.json` is touched.

### 1. Scope / Trigger

Applies when adding/altering a `task_*` command, the `TaskStatus` state machine, the
`tasks.json` on-disk shape, or anything in `src/stores/task.ts` / `taskService.ts`.

### 2. Signatures (all camelCase results, `Result<T, String>`)

```rust
task_list          { projectId?: string }                    -> WorkTask[]
task_create        { draft: TaskDraft }                      -> TaskDetails { task, events }
task_update        { id, patch: TaskPatch }                  -> TaskDetails   // todo/failed only
task_delete        { id }                                    -> ()
task_transition    { id, to, expectedFrom }                  -> TaskDetails
task_reorder       { projectId, orderedIds: int[] }          -> WorkTask[]    // project's rows
task_attach_session{ id, sessionId, engineId? }              -> TaskDetails
task_archive       { id, archived: bool }                    -> TaskDetails   // terminal only
task_events        { id }                                    -> WorkTaskEvent[]
task_settle_turn   { id, runSeq, outcome, reason? }          -> WorkTask      // sub-task 3
task_detach_session{ id, sessionId }                         -> WorkTask      // sub-task 3
task_duplicate     { id }                                    -> TaskDetails   // sub-task 3
```

- `TaskDetails` (`{task, events}`) is the create/update/transition return shape: the
  detail sheet needs the timeline on the same round trip, so no second call.
- `task_events` on an unknown id returns `[]`, not `not-found` — a concurrently deleted
  task must not surface as an error in a dialog that is just closing.
- Timestamps are **`i64` milliseconds**; the crate does not depend on `chrono`.

### 3. Contracts

- **File**: `~/.lightr-vibe/lightr-vibe/tasks.json` (app-namespace hard rule above).
  `{ "version": 1, "tasks": [...], "events": [...] }`. Events are kept ascending and
  capped at **200 per task**, trimmed on both `push_event` and `save_serialized`.
- **State machine** is the single authority (`TRANSITIONS` const table +
  `can_transition(from, to)`; `design.md` §3.1 of task 10-17-task-board). `done` and
  `merging` have **no** out-edges. `review → todo` (打回) exists here and does not exist
  in codeg.
- **Transition side effects**: entering `queued` → `runSeq += 1` and `startedAt` if null;
  entering `review`/`failed` → `settledAt`; entering `done`/`canceled` → `finishedAt`.
  Every accepted transition appends a `status_changed` event with `payload {from, to}`.
- **Error strings** (frontend classifies by this exact vocabulary):
  `not-found` · `stale` · `illegal-transition: <from>-><to>` · `io: <message>`.
  Business rejections that are not about identity or ordering (e.g. archiving a
  non-terminal task) use the `io:` prefix — it is the only code that carries a message.
- **CAS**: `expectedFrom` is compared **before** the table lookup and **before** any
  event is pushed, so a stale write mutates nothing at all.
- **`review|failed → todo` (打回) clears `settledAt`; `queued → todo` and `canceled → todo`
  do NOT.** The clear is conditional on the *source* status (`apply_entry_effects`
  takes `from`). An unconditional clear would erase a real settlement time when a
  failed start chain rolls a task back to `todo`.

### Task ↔ session turn flow-back (task 10-17-task-review-session)

`task_settle_turn` is the **only** way a task leaves `running` without a user action:

| Field | Contract |
|---|---|
| `outcome = ok` | `running → review`, clears `lastError` / `failureReason` |
| `outcome = empty` | `running → failed`, `failureReason = empty_turn`, `lastError` = caller's text (or a default) |
| `outcome = error` | `running → failed`, `failureReason = agent_error`, `lastError` = caller's text |
| `outcome = cancelled` | no-op (user stopped; the task stays `running` until the user approves/rejects) |
| `runSeq ≠ row.runSeq` | late callback → returns the row **unchanged** (never an error) |
| row status ≠ `running` | conclusion is obsolete (user already rejected/canceled) → unchanged |

- The settle transition and the failure attribution are written in **one** write
  transaction; the appended `status_changed` event has `actor: "engine"`.
- `TaskTurnOutcome` is `snake_case` on the wire (`ok` / `error` / `empty` / `cancelled`).
- `task_detach_session` is CAS on the current `sessionId` (`stale` on mismatch) and
  refuses while `queued|preparing|running` (`io: ...`); `task_duplicate` re-creates
  the row through `task_create` (never copies runtime fields).

Frontend side, the whole orchestration lives in **one** feature module,
`src/views/tasks/taskSessionBridge.ts` (not a composable — it owns a module-level
subscription, not component state):

- `startTask(id)` = the single start chain (card / detail / retry / drag-drop):
  `todo|failed → queued` → reuse `sessionId` or `createDraft` → `attachSession` →
  `queued → preparing → running` → `openSession` → `send(prompt)`. Failures roll back
  **by stage** (`rollbackStart`): `queued → todo` for an attach/preparing failure,
  `preparing → failed` for a running failure — no step may leave a row stuck in
  `queued`/`preparing`. The enqueue step itself touches nothing, so its error is
  rethrown untouched. An orphan task (project removed) is refused before any write.
  `runSeq` comes from the `queued` transition's return value.
- `onTurnSettled(fn)` (in `useConversation.ts`) is an optional **module-level** hook.
  With no listener registered it is a no-op and `send()` behaves exactly as before
  (AC7). `main.ts` registers the bridge once at boot — a turn can finish on the
  coding page after the board unmounted, so the subscription must not depend on
  `TasksView` being mounted.
- The listener only reacts to a task whose `sessionId` matches **and** whose status is
  `running` **and** whose `runSeq` still equals the token captured at start, and whose
  `startedAt` is not older than that token. Everything else is ignored silently.
- **`ok` is not enough (R4).** `useConversation` leaves `ok: true` for
  `stopReason === 'cancelled'`, so the bridge only settles `ok → review` when the turn
  actually produced content (`turnHasContent`: an assistant message created at/after
  `startedAt` with non-empty text/thinking, tool calls, or an error card). `ok` + zero
  content = the user pressed 停止 → the token is dropped and the task **stays
  `running`**. `empty`/`error` go through `task_settle_turn` with the token's
  `runSeq`; Rust answers a stale `runSeq` with the row unchanged.
- Browser-dev `simulateTurnSettle(task, 'ok' | 'stop' | 'empty' | 'error')` feeds the
  **same** `handleTurnSettled` path; the detail dialog seeds one assistant text
  message before `'ok'` so the simulation matches a real successful turn.

### Worktree commands (`src-tauri/src/worktree.rs`, task 10-26-task-worktree)

Optional git isolation for tasks. **Every git invocation in this repo lives here** —
no git call in services, stores, or components. Commands (camelCase, `Result<_, String>`):

```rust
worktree_repo_info { rootPath } -> { isRepo, currentBranch, branches, detail }
worktree_create    { req }      -> { worktreePath, workBranch, baseSha, baseBranch, initError? }
worktree_remove    { rootPath, worktreePath, workBranch, baseBranch?, baseSha? } -> ()
```

Task row gains `useWorktree` / `worktreePath` / `workBranch` / `baseBranch` / `baseSha` /
`initCommand` (all `#[serde(default)]`, so old `tasks.json` still parses). `TaskDraft` /
`TaskPatch` accept **only** `useWorktree` / `baseBranch` / `initCommand` — path, branch
name and sha are written by the creation flow and must not be forgeable by a client.

**Three traps, each cost a real bug somewhere:**

1. **Absolute paths only.** Git resolves a `<worktree>` argument by unique path
   **suffix** first and only then as a path, so a relative name can reach a
   registered worktree anywhere on disk and `--force` deletes it. `require_absolute`
   is the first statement of both create and remove; drive-relative (`C:foo`, `C:`)
   and non-UTF-8 paths are refused **before any git call**.
2. **`base_sha` before `worktree add`.** Recorded via `git rev-parse <base_branch>`
   before the checkout exists, so a branch switch mid-creation cannot drift the diff
   baseline.
3. **`worktree add -b <work_branch> <path> <base_branch>`** — not
   `add <path> <branch>`. The task branch is new; without `-b` git answers
   `fatal: invalid reference`, and with a same-name tag it silently produces a
   **detached** checkout. `-b` also pins the start point to `base_branch`.
   **Its side effect must be handled**: git creates the branch *before* the
   directory, so a directory-stage failure leaves the branch behind and every retry
   then fails with `a branch named ... already exists`. Roll back that branch on
   failure — only when it did not exist before the call, so a user's own branch is
   never deleted.
4. **Verify attachment with the full ref.** `git -C <abs> symbolic-ref --quiet HEAD`
   must equal `refs/heads/<work_branch>` (**not** `--short`: a same-name tag
   disambiguates to `heads/<name>`). On mismatch: remove the worktree, prune, delete
   the branch only if `branch_holds_unlanded_work` is false, then error.

Removal keeps a branch that still holds unlanded commits (tip is neither an ancestor
of base nor tree-equal to base); a probe error reads as "holds work" because deletion
is the irreversible half.

**Start chain (`taskSessionBridge.ts::startTask`)** — the task row's `useWorktree` decides
the session cwd:

```
let cwd = project.rootPath                 // default; switch OFF never touches git
if (queued.useWorktree) {                  // the ONLY place that calls worktreeService.create
  const info = await worktreeService.create({ rootPath, baseBranch, taskId, initCommand })
  await taskStore.attachWorktree(id, info)
  cwd = info.worktreePath
}
sessionStore.createDraft(model, { ..., cwd })
```

- **OFF must stay byte-identical**: same `cwd`, same transition order, zero `await` added,
  zero `worktreeService` calls. Stale-path cleanup is gated on `useWorktree` too, otherwise a
  row with the switch off would still run `git worktree remove` at launch.
- **Create fails** → task must reach `failed` **without** a draft/session. `queued → failed`
  is not in `TRANSITIONS`, so the legal hop is `queued → preparing → failed`; if the hop
  itself fails, fall back to `queued → todo` rather than stranding the row in `queued`
  (which permanently disables the start button).
- **Create succeeded but a later step fails** → remove the worktree **before** rolling the
  status back. The remove takes the freshly-returned facts (`WorktreeFacts`), not the store
  row — a write-back failure leaves the row stale and an empty `workBranch` would send git a
  bogus argument. A remove failure is noted with the path and the original cause is rethrown
  unchanged (never masked).
- **`initCommand` failure never blocks** — the note goes to `lastError` only.

**Terminal cleanup** is ONE helper (`cleanupWorktreeOnFinish`) called from every path that
reaches `done`/`canceled`: `approveTask`, `stopTask`, and the board's drag-to-terminal
(`transitionTask`). `review → todo` (reject), `duplicate`, and `settleTurn → review|failed`
must NOT clean — the worktree is still in use. On removal failure the status is **not**
rolled back; `lastError` carries the path and the detail dialog offers 重试清理.

**`task_worktree_issue { id, note }`** exists because `lastError` has no other writable
channel outside `todo`/`failed` (`task_update` is status-gated, `task_settle_turn` owns the
status machine). It writes **only** `lastError` + `updatedAt` — never status, verdict,
`runSeq`, or worktree fields — so it cannot bypass the state machine.

**`setup_error` is encoded, not enumerated**: `TaskFailureReason` has no such variant, and
extending it would need a migration for a purely presentational distinction. The convention
is `status: failed` + `lastError` starting with `准备失败：`; the chip checks that prefix
**before** switching on `failure_reason`, so `interrupted` / `agent_error` / `empty_turn`
rendering is untouched. The prefix constants live in
`components/setupErrorPrefix.ts` — a pure module, so the presentational chip does not import
the bridge (which would close a `bridge → session store → … → chip` cycle).

### 4. Validation & Error Matrix

| Condition | Behavior |
|---|---|
| File absent | Empty data, no error, no file created until the first write |
| JSON corrupt | Read returns empty data + warn; the damaged text is copied to `tasks.json.bak` **before** the next write overwrites it (never after) |
| `expectedFrom` ≠ row status | `stale`, row and events untouched |
| Edge not in `TRANSITIONS` | `illegal-transition: <from>-><to>`, untouched |
| `task_update` on `running` | `io: ...` rejection (only `todo`/`failed` are editable) |
| `task_archive` on non-terminal | `io: ...` rejection |
| `task_reorder` ids from another project | Silently ignored — no error, no change (ordering is project-scoped) |
| Unknown top-level key in `tasks.json` | Preserved across a read/write round trip |

### 5. Good/Base/Bad Cases

- Good: store action → `taskService` (`isTauri()` branch) → `invoke('task_*')` → typed
  camelCase `TaskDetails` → store replaces that row → board re-renders.
- Base: `transition()` returns the new row; the store never guesses the new status.
- Bad: a component computing the "next status" itself and writing it with `task_update`;
  a store doing an optimistic status write before `await`; anyone calling `invoke`
  outside `src/services/taskService.ts`.

### 6. Tests Required

- `TRANSITIONS` exhaustiveness: all 10×10 pairs asserted, and `TRANSITIONS.len()` equals
  the status count, so adding a status without filing it fails.
- `illegal-transition` strings for `todo->review` and `done->todo`; `stale` writes no event.
- `task_reorder` through the real command with foreign ids.
- 250 transitions → 200 events.
- Corrupt file: `.bak` written **before** the overwrite (ordering assertion, not just
  "a .bak exists").
- Command-layer tests must serialize: the store is a process-global singleton, so tests
  go through `lock_store()` / `point_store_at()` / `reset_store()` helpers. Without this
  they interleave and fail at random.

### 7. Wrong vs Correct

#### Wrong

```rust
// 先覆盖再备份：.bak 里是刚写的新内容，损坏的原文永久丢失
save_serialized(&store, &next)?;
backup_corrupt(&store.path, &snapshot.raw)?;
```

#### Correct

```rust
// 备份发生在覆盖之前，且备份失败不阻断写入
if snapshot.corrupt {
    backup_corrupt(&store.path, &snapshot.raw);
}
save_serialized(&store, &next)?;
```

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
