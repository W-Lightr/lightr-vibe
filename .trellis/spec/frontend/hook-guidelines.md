# Hook (Composable) Guidelines

> Vue composables (`useXxx`) in `src/composables/`. This project calls them
> composables; the Trellis template says "hooks" — same thing.

---

## When to write a composable

Write a composable when logic needs **Pinia stores + async control flow** shared
by multiple components (e.g. sending messages, streaming updates). Keep these
in `src/composables/`:

| Composable | Responsibility |
|---|---|
| `useConversation.ts` | Send user message, stream mock events into the session store, stop generation |
| `useConversationScroll.ts` | Scroll-position memory per session, follow-bottom behavior |

Rules of thumb observed in the codebase:

- Pure functions with no Vue reactivity → `src/utils/` instead (`time.ts`, `id.ts`).
- State shared app-wide → a Pinia store, not a composable-owned `ref`.
- A composable may **own module-level non-reactive state** when it must
  outlive component remounts (see below).

---

## Shape

Plain exported function, no default export, no class:

```ts
// src/composables/useConversation.ts
import { conversationService, type MockEvent } from '../services/conversationService'
import { useSessionStore } from '../stores/session'
import { useWorkspaceStore } from '../stores/workspace'

// Module-level: one AbortController per session, survives component remounts
const controllers = new Map<string, AbortController>()

export function useConversation() {
  const sessionStore = useSessionStore()
  const workspace = useWorkspaceStore()

  async function send(sessionId: string, content: string) { ... }
  function stop(sessionId: string) { ... }

  return { send, stop }
}
```

Conventions:

- **Stores are instantiated inside the function body** (`useSessionStore()`),
  not at module level — Pinia must be active first.
- **Return a plain object of functions/refs.** Return only what consumers need.
- Arguments are primitives or refs passed by the caller
  (`useConversationScroll(elRef: Ref<HTMLElement | null>)`) — composables don't
  reach into the DOM via globals.
- Internal helpers (`applyEvent`, `isNearBottom`) stay private, not returned.

---

## Module-level state for cross-mount persistence

When state must survive component unmount/remount (tab switching), hoist it to
module scope outside the composable — this is an established pattern, not a
smell:

```ts
// src/composables/useConversationScroll.ts
// 模块级存储：会话切换导致组件重挂载时滚动位置不丢失
const positions = new Map<string, number>()

export function useConversationScroll(elRef: Ref<HTMLElement | null>) {
  const atBottom = ref(true)
  ...
  return { atBottom, showBackToLatest, restore, followIfAtBottom, scrollToBottom }
}
```

Same for `controllers` in `useConversation.ts`. Key such maps by
`sessionId` / stable string keys.

---

## Async & cancellation

- Long-running per-entity operations carry an `AbortController` keyed by entity
  id (`controllers: Map<sessionId, AbortController>`).
- The service layer accepts `{ signal }` and the composable creates/aborts it —
  see `conversationService.send({ ..., signal })` and `stop(sessionId)`.
- Guard re-entry: `if (!content.trim() || workspace.isGenerating(sessionId)) return`.
- DOM-dependent async steps `await nextTick()` before measuring
  (`followIfAtBottom`, `restore` in `useConversationScroll`).

---

## Event-stream consumption

Streaming updates flow **service → composable → store**, never service → component:

1. `conversationService.send()` emits typed `MockEvent` union objects via an
   `onEvent` callback (`thinking-start` / `thinking-delta` / `tool-start` / ...).
2. `useConversation.applyEvent()` switches on `event.kind` and calls granular
   store mutators (`sessionStore.pushMessage`, `appendContent`, `patchMessage`,
   `patchToolCall`).
3. Components only read store state.

When wiring a real backend (SSE), preserve this seam: replace the mock event
producer inside `conversationService`, keep `MockEvent`-style typed events and
`applyEvent` mapping intact.

---

## Naming

- File and function share the name: `useConversationScroll.ts` → `export function useConversationScroll(...)`.
- camelCase, `use` prefix, verb-ish domain suffix. No `useXxxHelper`/`useXxxLogic` filler words.
