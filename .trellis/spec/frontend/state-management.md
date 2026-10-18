# State Management

> Pinia setup stores in `src/stores/`. Three stores split by concern.

---

## Store inventory & responsibilities

| Store | File | Owns |
|---|---|---|
| `useProjectStore` | `stores/project.ts` | Projects (workspaces): file trees + mock in-memory file contents; `TMP_PROJECT_ID` temp-session project |
| `useSessionStore` | `stores/session.ts` | Sessions and their messages: CRUD, message push/append/patch, tool-call patching, token usage, status |
| `useWorkspaceStore` | `stores/workspace.ts` | UI state: open tabs, active tab, panel visibility, pane widths, model/execution-mode selection, generating-session set |

Boundaries:

- **Domain data** (sessions, messages, projects) lives in its domain store.
- **Transient UI state** (which tab is active, sidebar open, pane widths) lives
  in `workspace`.
- Cross-store effects are done by the caller/composable, not by stores importing
  each other: e.g. deleting a session → `sessionStore.deleteSession(id)` **then**
  `workspace.forgetSession(id)` (each store cleans its own slice).
- `workspace` also exports shared constants used by components:
  `MODELS`, `EXECUTION_MODES` (module-level `as const` / typed arrays).

All current state is **in-memory mock data** (seeded projects, seeded session).
Persistence (Tauri fs) is a future swap; keep the store API stable so the swap
happens behind store functions.

---

## Setup-store style (mandatory)

All stores use the **setup (composition) style** — a function returning refs,
computed, and functions. Never use the options style (`state/getters/actions`
object).

```ts
// src/stores/workspace.ts
import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { ExecutionMode, OpenTab } from '../types/session'

export const useWorkspaceStore = defineStore('workspace', () => {
  // state
  const tabs = ref<OpenTab[]>([])
  const activeTabId = ref<string | null>(null)
  const leftWidth = ref(260)

  // getters
  const activeTab = computed(() => tabs.value.find((t) => t.id === activeTabId.value) ?? null)
  const activeSessionId = computed(() =>
    activeTab.value?.kind === 'session' ? (activeTab.value.sessionId ?? null) : null,
  )

  // actions
  /** 打开会话标签（已打开则复用），并设为活动标签 */
  function openSession(sessionId: string) { ... }
  function closeTab(tabId: string) { ... }
  function isGenerating(id: string | null | undefined): boolean { ... }

  // explicit return — every member consumers may use
  return {
    tabs, activeTabId, activeTab, activeSessionId,
    leftWidth, rightWidth, sidebarVisible, inspectorVisible,
    openSession, openFile, closeTab, forgetSession, setGenerating, isGenerating,
  }
})
```

Conventions inside a setup store:

- State = `ref<T>(...)` with explicit type args (`ref<string | null>(null)`,
  `ref<OpenTab[]>([])`).
- Derived = `computed(...)`.
- Mutations = plain `function` declarations with a `/** ... */` JSDoc (Chinese)
  describing intent.
- The final `return` lists exposed members explicitly. Do not return internal
  helpers (e.g. `session.ts` keeps `seedFolderSession` module-private).
- Store id string matches the file name: `defineStore('workspace', ...)`.

---

## Lookup helpers

Stores expose **`get`-style accessors** rather than making consumers filter
arrays. `session.ts` builds an index for O(1) lookups (and, since the
new-session-flow task, also consults the draft staging area — see below):

```ts
const byId = computed(() => {
  const map = new Map<string, Session>()
  for (const s of sessions.value) map.set(s.id, s)
  return map
})
function get(id: string | null | undefined): Session | null {
  return id ? (byId.value.get(id) ?? drafts.get(id) ?? null) : null
}
```

- Accept `string | null | undefined` and return `T | null` — callers pass
  `workspace.activeSessionId` straight through without null-guarding first.
- Granular mutators instead of "replace whole object":
  `pushMessage`, `appendContent`, `patchMessage(sessionId, messageId, Partial<Message>)`,
  `patchToolCall(sessionId, messageId, toolCallId, Partial<ToolCall>)`.

---

## Draft staging (new sessions, task 10-13-session-create-flow)

`useSessionStore` separates **persisted sessions** from **unsent drafts**:

| Member | Meaning |
|---|---|
| `sessions: ref<Session[]>` | The listed/persisted array. **Only** source for the sidebar (`folderGroups` / `tmpSessions` / count) and for `persistIndex` (index JSON). |
| `drafts: Map<string, Session>` | Module-in-store staging for new sessions that have not been sent yet. NOT reactive-listed, NEVER serialized. |

Rules:

- **`createDraft(model, opts)`** builds the session (title placeholder exactly
  `'新会话'` — NOT length-numbered, so multiple drafts don't collide), sets
  `messagesLoaded`, cwd/storagePath fallbacks, and `drafts.set`. It must **not**
  `sessions.value.unshift` and must **not** `schedulePersistIndex`.
- **`get(id)`** consults `sessions` then `drafts` (see snippet above), so the UI
  (tab / composer / conversation view) can render a draft by id.
- **`promoteDraft(id)`** = `drafts.delete` + `sessions.value.unshift` +
  `schedulePersistIndex`. Called by `useConversation.send()` **before**
  `pushMessage`, so 「首次发送 = 入列 + 落盘」. `applyAutoTitle` then runs and
  matches the `'新会话'` placeholder.
- **`discardDraft(id)`** = `drafts.delete(id)` (idempotent). Triggered
  unconditionally on **tab close** (`SessionTabs`) and on **switch-away** only
  when the composer input was empty (`MessageComposer` watch). Discarding a draft
  must also `closeConnection(id)` (kill the ACP process opened by
  auto-connect) and clear its draft-input entry.
- **Draft input memory** lives in a module-level `Map<sessionId, string>` in
  `useConversation.ts` (exported `getDraftInput/setDraftInput/clearDraftInput`),
  following the cross-mount pattern; both the composer and tab-close clear it.
- `forkSession` / `seedFolderSession` keep using the original eager
  `createSession` (they are not drafts). `SessionIndexEntry` schema is unchanged —
  drafts simply never persist.

> **Why**: a brand-new session must not appear in the list or on disk until the
> user actually sends a message; an abandoned draft vanishes on tab close. Keeping
> drafts out of `sessions` makes both the list and persistence "not see" them for
> free; `get()` is the single seam that lets the UI resolve a draft by id.

---

## IDs & tab keys

- Entity IDs come from `createId(prefix)` (`utils/id.ts`): `ses-xxxxxxxx`,
  `msg-...`, `tool-...`, `ext-...`.
- Tab IDs are **derived from their target**, so re-opening reuses the tab:
  session tab → `tab-ses-${sessionId}`, file tab → `tab-file-${filePath}`
  (see `openSession` / `openFile` in `workspace.ts`).

---

## Using stores in components

```ts
const workspace = useWorkspaceStore()
const sessionStore = useSessionStore()
```

- Instance variable is named after the store (`workspace`, `sessionStore`,
  `projectStore`) — match the existing name in the file you are editing.
- Simple state writes may happen directly in templates
  (`@click="workspace.activeTabId = tab.id"`); anything with logic (reuse,
  cleanup, side effects) must go through a store function.
- Generation flag pattern: `workspace.setGenerating(id, bool)` +
  `workspace.isGenerating(id)` — a session id set, so multiple sessions can
  generate concurrently while UI queries per-id.

---

## Async flow ownership

- Stores stay **synchronous** except where noted; async orchestration (send /
  stream / abort) lives in `composables/useConversation.ts`, which calls
  `conversationService` and pushes results into `sessionStore` via its
  granular mutators.
- Do not add `async` actions that call services directly inside stores; keep
  the composable seam.

**Exception: the task store (`stores/task.ts`, task 10-17-task-data-layer).**
Its mutations are `async` and call `taskService` directly. This is deliberate:
the Rust side owns the state machine, so every mutation's result — the new row
plus its timeline — is the only valid source of truth. Conditions:

- **No optimistic updates.** Always `await` the service, then replace that row
  with what came back. The frontend never invents a status or a timestamp.
- Failures propagate to the caller (the view shows the error); the store does
  not swallow or retry.
- `load()` is idempotent and guarded by `loaded`, so mounting a view twice does
  not re-read the file.
- Read-side helpers (`byProject`, `eventsOf`) stay synchronous over the loaded
  array.

### Turn flow-back into tasks (task 10-17-task-review-session)

`taskStore.settleTurn(id, runSeq, outcome, reason)` / `detachSession` / `duplicate`
follow the same async rule (await the service, replace the row). They are called by
`views/tasks/taskSessionBridge.ts`, which subscribes **once** (from `main.ts`) to
`onTurnSettled()`:

- match the task **by `sessionId`** and require `status === 'running'`;
- require the `runSeq` captured at start (`transition → queued` return) and a
  `startedAt` no older than that start token — otherwise the callback is stale and
  is dropped;
- `ok → review`, `empty → failed(empty_turn)`, any other error `→ failed(agent_error)`;
  Rust validates the same `runSeq` again, so a stale callback is a no-op server-side
  too (it returns the row unchanged, never an error).

