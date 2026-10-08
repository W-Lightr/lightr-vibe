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
arrays. `session.ts` builds an index for O(1) lookups:

```ts
const byId = computed(() => {
  const map = new Map<string, Session>()
  for (const s of sessions.value) map.set(s.id, s)
  return map
})
function get(id: string | null | undefined): Session | null {
  return id ? (byId.value.get(id) ?? null) : null
}
```

- Accept `string | null | undefined` and return `T | null` — callers pass
  `workspace.activeSessionId` straight through without null-guarding first.
- Granular mutators instead of "replace whole object":
  `pushMessage`, `appendContent`, `patchMessage(sessionId, messageId, Partial<Message>)`,
  `patchToolCall(sessionId, messageId, toolCallId, Partial<ToolCall>)`.

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
