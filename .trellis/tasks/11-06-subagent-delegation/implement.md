# Implementation Plan：通用子代理委派与会话展示

## Preconditions

- This task remains in `planning` until the user approves this complete plan.
- Implementation follows the frontend/Tauri bridge specifications and does not introduce a new dependency unless a documented constraint requires it.
- No existing user changes may be reverted; work starts from a clean or carefully preserved working tree.

## Phase 1 — Contracts and durable state

1. Add `src-tauri/src/delegation.rs` with typed task records, settings, error/status vocabulary, app-namespace persistence and startup recovery of nonterminal records.
2. Add Tauri commands for reading/updating delegation settings and listing/restoring task records; register them in `src-tauri/src/lib.rs`.
3. Add `src/types/delegation.ts`, `src/services/delegationService.ts`, `src/services/delegationMock.ts`, and `src/stores/delegation.ts` with matching camelCase contracts.
4. Extend `Session` / session index persistence with optional parent/origin fields and add granular store helpers to create and restore delegated children without exposing them in ordinary sidebar groups.
5. Validate: Rust serialization/recovery tests; `pnpm build`; existing session persistence smoke check.

## Phase 2 — Broker and ACP integration

1. Implement `DelegationBroker` behind narrow ACP lifecycle methods rather than accessing `CONNS` directly.
2. Enforce global enabled setting, engine validation, cwd containment, parent ownership, depth limit and sibling isolation.
3. Allocate/persist child task state before spawn; emit typed `delegation://started` before child `session/new`; asynchronously run connect/new/prompt.
4. Translate child terminal, permission and question changes into task lifecycle updates. Ensure parent disconnect/cancel drains only direct active children and produces terminal cancellation reports.
5. Validate: fake child runtime tests for happy path, invalid path/engine/depth, failure, cancellation, parent teardown and restart interruption.

## Phase 3 — Companion MCP controls

1. Generalize `src-tauri/src/bin/lightr_ask_mcp.rs` to support tagged socket requests while retaining the existing `ask_user_question` protocol and tests.
2. Add schemas/handlers for `delegate_to_agent`, `get_delegation_status` and `cancel_delegation`; responses use structured task reports plus concise Agent-readable text.
3. Extend `src-tauri/src/acp.rs` socket handler and `mcp_servers_for` to register delegation tools only when enabled; preserve existing ask-tool injection and cancellation cleanup.
4. Make active-session reconnect behavior explicit when settings change; ensure no disabled session advertises delegation tools.
5. Validate: stdio MCP framing tests, token/connection ownership rejection tests, and an ACP probe showing the expected tool set with toggle off/on.
6. Make `lightr-ask-mcp` a deterministic dev and packaged artifact; add a staged-binary `tools/list` smoke test to prevent stale companion regressions.

## Phase 4 — Frontend cards, event routing and viewer

1. Subscribe once to delegation lifecycle events alongside existing ACP listeners; update the delegation store and create the delegated child Session on `started`.
2. Extend tool-call normalization with structured delegation metadata and parsers; preserve generic rendering for unknown or malformed MCP payloads.
3. Add `DelegationCard`, `DelegationStatusCard`, status badge/parser utilities and route them from `ToolCallPart` / `ContentPartView`.
4. Build a nested right-side child-session drawer reusing the existing transcript display. Ensure permission and question queues filter by child local `connId`.
5. Add collaboration settings UI and explain default-off/direct-after-enable/reconnect behavior.
6. Validate: browser mock interactions, TypeScript build, Tauri manual path for card, drawer, permissions and questions.
7. Create/reconcile the parent delegation card from `delegation://started` / persisted records, independent of adapter tool titles; await listener readiness before the app mounts or opens ACP sessions.

## Phase 5 — Codex native compatibility and quality gate

1. Add an isolated Codex collaboration tool parser and map only verified fields to the generic visual model.
2. Render a non-clickable summary when no stable child-session reference exists; retain generic tool fallback for unknown payload versions.
3. Execute full regression checks: ordinary conversations, permissions, elicitation, session close/reconnect, all three engine list behavior and task-board session interactions.
4. Run `pnpm build` and `cargo check`; run relevant Rust tests; manually exercise desktop Tauri flow. Record results in the task artifacts.

## Risk Checkpoints / Rollback

| Checkpoint | Risk | Stop / rollback criterion |
|---|---|---|
| Phase 2 | ACP lifecycle coupling can leak child processes or break parent sessions | Do not proceed until child cancel/disconnect is deterministic in tests |
| Phase 3 | Companion protocol change can regress `ask_user_question` | Keep the existing ask behavior under explicit compatibility tests before enabling delegation tools |
| Phase 4 | Child events can arrive before frontend session registration | Require started-before-spawn ordering and tolerant event buffering/creation |
| Phase 5 | Codex payload drift | Keep native compatibility isolated and optional; never block the MCP universal flow |

Rollback is feature-level: set delegation disabled / remove its MCP injection. Existing session and message persistence remains backwards readable because parent/origin and delegation metadata are optional.

## Final Quality Gate

- [ ] `pnpm build`
- [ ] `cd src-tauri && cargo check`
- [ ] Rust delegation and companion-MCP tests
- [ ] Browser mock smoke path
- [ ] Desktop Tauri manual flow with concurrent delegation, child interaction, cancel, close and restart recovery
- [ ] Review existing single-agent ACP behavior for regression
