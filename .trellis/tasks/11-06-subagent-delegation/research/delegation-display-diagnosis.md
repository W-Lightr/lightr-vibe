# Research: delegation display diagnosis

- Query: Why does a requested child-agent delegation produce neither a parent card nor an automatically opened right-side child viewer?
- Scope: internal
- Date: 2026-11-06

## Findings

### Primary blocking cause: the MCP companion actually launched in dev is stale

The delegation controls live in the separate `lightr-ask-mcp` executable, not
in the Tauri main executable.  The current source correctly parses
`--delegation-enabled`, conditionally returns the three controls from
`tools/list`, and forwards a `delegate` socket request:

- `src-tauri/src/bin/lightr_ask_mcp.rs:60-85` parses the flag.
- `src-tauri/src/bin/lightr_ask_mcp.rs:106-128` advertises and dispatches the
  three delegation tools.
- `src-tauri/src/acp.rs:964-979` injects the *sibling executable* returned by
  `ask_bin_path()` into every ACP `session/new`/`session/load` request.

However, the executable present at the precise path used by dev is
`src-tauri/target/debug/lightr-ask-mcp.exe`, timestamped 2026-10-27 13:59,
while its source was modified 2026-11-06 13:03.  The main executable is newer
(2026-11-06 14:00).  Directly running the installed debug companion with a
JSON-RPC `tools/list` request and `--delegation-enabled true` returned only
`ask_user_question`; it did **not** return `delegate_to_agent`,
`get_delegation_status`, or `cancel_delegation`.  The same tool list resulted
with `false`.

This is consistent with the build wiring:

- `src-tauri/Cargo.toml:7` sets `default-run = "lightr-vibe"`; the companion
  is merely another `[[bin]]` at lines 20-23.
- `tauri-dev.bat:9` runs `pnpm tauri dev` only; it contains no explicit
  `cargo build --bin lightr-ask-mcp` step.
- `src-tauri/tauri.conf.json:29` packages only
  `binaries/agent-sidecar` as `externalBin`; it has no rule for the companion.
- No release `lightr-ask-mcp.exe` or release main executable exists in this
  checkout, so the release packaging path has not been exercised either.

Therefore the ACP engine cannot discover or execute the current delegation
MCP controls in the tested dev process.  Without a valid MCP `delegate` call,
the broker is never reached, no `delegation://started` event is emitted, and
neither display path can activate.  Supporting local evidence: the live app
data directory has no `delegation-settings.json` or `delegations.json`, which
the broker would create when the setting or a delegation record is persisted;
the available app log is old and has no ACP/delegation entries.

### Intended runtime path is otherwise connected

For a current companion executable, the complete path is:

1. `mcp_servers_for()` passes its path, loopback address, per-connection token,
   connection id, and current enabled state to the ACP engine
   (`src-tauri/src/acp.rs:964-979`).
2. The companion forwards `kind: "delegate"` through the authenticated local
   socket (`src-tauri/src/bin/lightr_ask_mcp.rs:121-127, 203-230`).
3. `handle_ask_connection()` obtains the parent connection/cwd and calls
   `delegation::delegate()` (`src-tauri/src/acp.rs:985-1045`).
4. The broker persists the record, emits `delegation://started`, then spawns
   the independent child ACP session (`src-tauri/src/delegation.rs:176-217`).
5. Tauri event listeners forward the event into the frontend dispatcher
   (`src/services/acpService.ts:49-70`), and `main.ts:58-73` registers the
   child session and calls `openDelegatedChild()`.  That function sets both
   `delegatedChildSessionId` and `inspectorVisible` (`src/stores/workspace.ts:59-62`).

So, once a genuine `delegate` reaches step 4 while the frontend is running,
the right viewer is designed to open without any card click.  The report of
both missing UI elements matches a failure before step 4, rather than a
drawer-only rendering error.

### Card display has a second, independent reliability gap

The parent card is **not** generated from the durable
`delegation://started` event.  It depends on the ACP adapter independently
emitting a `session/update` `tool_call` whose display `title` happens to end
in an accepted delegation tool name:

- `src/composables/useConversation.ts:583-623` copies only
  `update.title` into the stored tool `name`; it does not retain a machine
  tool-name field and drops `rawOutput`.
- `src/views/coding/components/ToolCallPart.vue:32,138` recognizes delegation
  only via the restrictive regex on that presentation title.
- `src/views/coding/components/DelegationCard.vue:14-38` derives the task id
  from the rendered tool output, then looks up the broker record.

ACP's normalized `AcpToolCallUpdate` has a `title` but no defined MCP-name
field (`src/types/acp.ts:38-52`).  Different ACP adapters can title an MCP
tool as prose, a namespaced tool, or omit the title.  In any of those cases a
delegation that succeeds at the broker may open the viewer yet render as a
generic tool card (or be absent if the adapter does not send MCP tool-call
updates).  This violates the requirement that the parent transcript show a
delegation card independently of adapter presentation choices.

### Delivery/packaging gap affects production too

`ask_bin_path()` assumes the companion sits beside the main executable
(`src-tauri/src/acp.rs:943-959`), whereas the current Tauri bundle configuration
does not place this second Cargo binary there (`src-tauri/tauri.conf.json:26-40`).
Even after fixing dev rebuilding, a packaged app will either omit it or fail
to locate it unless build/package staging and runtime path lookup are made
explicit.

## Minimal repair recommendation

1. Make the companion a first-class build artifact.  Add a deterministic
   pre-dev/pre-build step that builds `--bin lightr-ask-mcp`, and have the
   Tauri production bundle stage that exact executable as a resource/sidecar.
   Resolve it from the packaged resource/sidecar location in production and
   from `target/debug` in dev; do not rely solely on a sibling executable.
   Add a build smoke test that invokes the staged companion with
   `--delegation-enabled true` and asserts all four MCP tools are listed.
2. Add end-to-end diagnostic logging around `mcp_servers_for`, the socket
   request kind, broker rejection/success, and frontend receipt of
   `delegation://started`, with task id/connection id but no token.  This makes
   the next break visible at the exact boundary.
3. Make parent-card creation event-driven: on `delegation://started`, insert
   or upsert a typed delegation transcript item for `parentSessionId` using the
   durable record.  Preserve it across reload.  Treat ACP tool-call data only
   as supplementary execution detail, not the source of truth.
4. If retaining `ToolCallPart` routing, retain a stable tool identifier from
   the adapter (or use an explicit per-call correlation id) rather than
   matching the human-facing `title`; add adapter fixtures for actual MCP
   tool-call updates.
5. Await listener initialization during bootstrap (rather than fire-and-forget
   at `src/main.ts:89`) and rehydrate existing delegation records into child
   session projections at startup.  This closes the small event-registration
   race and lets a restart retain a usable viewer.

## Caveats / Not Found

- No fresh desktop session or delegation persistence/log record exists on this
  machine, so the diagnosis cannot identify a particular agent engine's exact
  emitted `tool_call.title`.  The direct companion `tools/list` reproduction
  is sufficient to establish the first, pre-broker failure.
- The existing log file contains only old unrelated entries and is not useful
  for this run.
- This investigation made no code or configuration changes.
