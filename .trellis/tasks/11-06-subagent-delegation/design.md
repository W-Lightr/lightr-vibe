# Design：通用子代理委派与会话展示

## 1. Design Goals

在不改变现有「每个 Session 一条独立 ACP 进程」模型的前提下，增加一个宿主控制面，让任何支持 MCP 的父 Agent 都能异步创建和管理子 Agent。ACP 不承担编排职责，只承载父/子各自的会话、流和交互。

```text
Parent ACP session
  │  delegate_to_agent / get_delegation_status / cancel_delegation
  ▼
local companion MCP (stdio + loopback-authenticated socket)
  ▼
Rust DelegationBroker
  ├─ validate engine, parent, cwd and depth
  ├─ persist DelegationRecord
  ├─ create child local session id
  ├─ start child ACP connection and prompt asynchronously
  └─ emit lifecycle events to the frontend
       ▼
Pinia delegation state + session store
  ├─ parent transcript delegation card
  └─ child-session side drawer
```

## 2. Boundaries and Responsibilities

| Layer | Owns | Does not own |
|---|---|---|
| Companion MCP | JSON-RPC MCP framing, tool schemas, loopback authentication, forwarding requests | ACP process management or UI state |
| `DelegationBroker` (Rust) | validation, lifecycle state, child connection spawn/cancel, durable task records, lifecycle events | rendering and parent transcript layout |
| existing `acp.rs` | one connection/process per child session, ACP request/response/update routing, permission and elicitation behavior | delegation-specific task semantics |
| frontend delegation service/store | Tauri event subscription, mock parity, card state projection, restoring stored records | direct `invoke` calls from components |
| session store | child `Session` creation/persistence and regular transcript state | broker runtime state |
| Vue components | cards, badges and nested right-side viewer | protocol parsing and lifecycle ownership |

## 3. Runtime Contract

### 3.1 MCP tools

The companion exposes these tools only when the persisted delegation setting is enabled:

| Tool | Required input | Response |
|---|---|---|
| `delegate_to_agent` | `agent_type`, self-contained `task`; optional `working_dir` | immediate `{ task_id, status: "running" }` or terminal setup failure |
| `get_delegation_status` | one or more `task_ids`; optional bounded `wait_ms` | ordered task reports |
| `cancel_delegation` | `task_id` | updated terminal/canceling report |

`task` must be self-contained. Child sessions do not receive parent history, open tabs or implicit user context. The broker records the parent local session id from the authenticated MCP connection; it never trusts a parent id supplied in tool arguments.

### 3.2 Delegation task record

Rust owns a durable `DelegationRecord`, stored only below the existing app namespace:

```text
~/.lightr-vibe/lightr-vibe/delegations.json
```

Essential fields are: `taskId`, `parentSessionId`, `childSessionId`, `agentType`, task preview, effective cwd, `status`, error code/message, created/updated timestamps, completion summary and optional parent-tool correlation once observable. Records never contain full parent transcript or secret config.

Status is a closed contract: `running`, `waiting_permission`, `waiting_question`, `completed`, `failed`, `canceled`, `interrupted`. On process restart every nonterminal record becomes `interrupted`; the app must never claim a terminated child is still active.

### 3.3 Spawn and lifecycle

1. The MCP server forwards a verified request to the broker.
2. The broker rejects disabled delegation, unknown/unavailable engines, invalid cwd, missing parent connection, or depth beyond the configured limit.
3. It allocates a local child session id and persists a `running` record before any child work starts.
4. It emits `delegation://started` before `session/new`, so the frontend creates/registers the child Session before it receives child ACP updates.
5. It invokes the existing ACP connect → session/new → prompt path using the child id and selected engine. The actual prompt execution runs independently of the MCP response.
6. It projects child terminal outcomes and pending permission/question states to `delegation://updated`; terminal states are persisted before emitting.
7. Parent cancel/disconnect cancels all direct running children. Canceling a task targets only that child.

The broker has a small explicit interface to `acp.rs`, rather than reaching into its mutable connection map. That interface must expose child start, cancel/disconnect and a subscription/callback for terminal outcomes. Existing parent event payloads retain their current shapes.

### 3.4 MCP companion evolution

`src-tauri/src/bin/lightr_ask_mcp.rs` becomes the generic local companion while retaining `ask_user_question` behavior. Its loopback message is changed from an ask-specific payload to a tagged request/response enum (`ask`, `delegate`, `status`, `cancel`), authenticated by the existing per-connection token.

`mcp_servers_for` injects the companion configuration only for the tools allowed by current settings. Changing the global toggle affects new or reconnected ACP sessions; the settings UI must say this explicitly and offer/recommend reconnecting active sessions. This avoids advertising a tool that will later be rejected.

The companion is a first-class build artifact: the dev command builds the
`lightr-ask-mcp` binary before launching Tauri, and the production build stages
the exact binary beside or as a resolved bundle resource. Runtime lookup uses an
explicit dev/resource resolution strategy rather than assuming a stale sibling
binary is current. A smoke check starts the staged companion with delegation
enabled and asserts the three delegation tool names appear in `tools/list`.

## 4. Frontend Design

### 4.1 Types and services

- Add a dedicated `types/delegation.ts` for wire-safe task reports and Tauri event payloads. Do not overload `Task` from the task-board domain.
- Add `services/delegationService.ts` and `delegationMock.ts`; both provide the same typed dual channel required by the Tauri bridge convention.
- Add a setup-style `stores/delegation.ts` for live reports keyed by `taskId`; it only projects broker state and does not own ACP messages.
- Extend `Session` with optional `parentSessionId` and `origin: 'user' | 'delegation'`. Sidebar selectors exclude delegated sessions by default while `get(id)` and tab opening still resolve them.
- Extend tool-call metadata only with structured delegation correlation needed after transcript reload. Unknown metadata remains tolerant.

### 4.2 Card and drawer UX

`ToolCallPart` routes the three MCP tool names to dedicated components instead of the generic tool shell:

- **delegation card**: engine icon/name, task first line, short task id, badge, failure reason and “查看会话”.
- **status card**: compact current/terminal outcome; consecutive polls for the same task merge rather than creating repeated large blocks.
- **cancel card**: outcome badge and cause.

The “查看会话” action opens a nested right-side drawer that renders the existing transcript components against `childSessionId`. It is available only where a real child Session exists. Permission and question queues already route by local `connId`; the drawer must select/filter by the child id, not the parent ACP session id.

No fake session link is rendered for Codex-native collaboration data that lacks a stable child session reference.

The durable `delegation://started` event is also the source of truth for an
event-driven parent transcript entry. It creates/updates a typed delegation
card for `parentSessionId` even when the ACP adapter omits an MCP tool-call
update or supplies a prose title. Adapter tool details may enrich that card but
never determine whether it exists.

### 4.3 Settings

Add an Agent/Collaboration settings section with:

- `enabled` switch, default `false`;
- max depth, default `1`, bounded to a small safe maximum;
- explanation that enabling delegates lets the parent Agent start local child sessions without per-delegation confirmation;
- a note that children still require individual permission/question responses and that toggling applies after reconnection.

## 5. Codex Native Collaboration Compatibility

Normalize known Codex collaboration tool-call names/payloads in a separate parser. Map available task text, model, agent id and status into the same card model. This code cannot create generic child sessions and must be optional: missing/changed Codex fields degrade to an ordinary tool call or read-only summary.

## 6. Safety, Compatibility and Rollback

- Validate cwd with the existing project-boundary semantics; a child may use only the parent project root or a normalized descendant. Temporary parents use their controlled temporary cwd.
- Delegation is off by default and not advertised to an Agent until enabled.
- Child prompts are self-contained; parent transcript is never copied automatically.
- No dependency downloads or shell installation is triggered.
- Every Tauri command has a browser mock equivalent; browser mode must simulate visible lifecycle states without starting processes.
- Rollback removes the MCP tool injection and UI entry points; existing ordinary ACP sessions, session indexes and messages remain readable because new fields are optional.

## 7. Verification Strategy

- Rust unit tests: validation, depth/cwd rejection, transitions, terminal persistence, parent cancellation cascade, restart interruption marking and MCP request routing.
- Rust integration tests with a fake ACP-facing child runtime: start → updates → permission/question → terminal; independent sibling cancellation isolation.
- Vue/type checks: typed service/mock parity, event projection, delegated-session sidebar exclusion, card state mapping and drawer availability.
- Manual Tauri run: two concurrent child sessions on available engines, child permission and question interaction, cancel, parent close, restart recovery, disabled setting and standard non-delegated conversation regression.
