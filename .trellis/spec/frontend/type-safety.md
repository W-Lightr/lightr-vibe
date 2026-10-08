# Type Safety

> TypeScript conventions. `tsconfig.json` runs with `strict: true`,
> `noUnusedLocals`, `noUnusedParameters`, `noFallthroughCasesInSwitch`.
> Type checking is part of the build: `pnpm build` = `vue-tsc --noEmit && vite build`.

---

## Domain model lives in `src/types/`

All core entities are declared once in `src/types/session.ts` and imported
everywhere as **type-only imports**:

```ts
import type { Session } from '../../../types/session'
import type { ExecutionMode, OpenTab } from '../types/session'
```

- Never re-declare or widen a domain shape inline in a component/store.
- New domain concepts get added to `types/session.ts` (or a sibling file in
  `types/`), not colocated in the first file that needs them.

---

## Type idioms used in the model

From `src/types/session.ts` — follow these exact idioms:

```ts
// String-literal unions for enums (no TS `enum`, ever)
export type MessageRole = 'user' | 'assistant'
export type MessageStatus = 'pending' | 'streaming' | 'done' | 'stopped' | 'error'

// Interfaces for entities; optional members use `?:` with a /** */ doc comment
export interface Message {
  id: string
  role: MessageRole
  content: string
  status: MessageStatus
  /** 同一次问答轮次的关联 ID */
  turnId?: string
  toolCalls?: ToolCall[]
}

// Discriminated unions for multi-kind entities
export interface OpenTab {
  id: string
  kind: 'session' | 'file'
  sessionId?: string
  filePath?: string
}
```

- **No `enum`** — string-literal union types (`MessageType`, `SessionKind`, ...).
- **No `any`.** Unknown-shaped data gets a declared interface or `unknown`.
- Numeric separators for large literals: `128_000`, `1_000_000`, `60_000`.
- JSDoc `/** ... */` (Chinese) on non-obvious interface members — they surface
  in editor hovers; keep doing this.

---

## Typing Vue constructs

Type-only generics, no runtime prop declarations:

```ts
const props = defineProps<{ session: Session; active: boolean }>()

const emit = defineEmits<{
  select: []
  'update:renameDraft': [value: string]
}>()
```

Refs that hold DOM elements or nullable state are explicitly parameterized:

```ts
const textareaEl = ref<HTMLTextAreaElement | null>(null)
const activeTabId = ref<string | null>(null)
```

DOM event handlers cast the target where needed (pattern from `SessionItem.vue`):

```ts
@input="emit('update:renameDraft', ($event.target as HTMLInputElement).value)"
```

---

## Typing services & events

The service layer exports its option/event types so consumers switch on them
exhaustively:

```ts
// services/conversationService.ts
export interface SendMessageOptions { ... }
export type MockEvent =
  | { kind: 'thinking-start'; messageId: string }
  | { kind: 'thinking-delta'; messageId: string; delta: string }
  | { kind: 'tool-start'; messageId: string; toolCall: ToolCall }
  | ...
```

`useConversation.applyEvent` switches on `event.kind` — the discriminated
union gives narrowing for free. When adding an event kind, extend the union;
the switch in `applyEvent` is the single mapping point.

---

## Nullability conventions

- Lookups return `T | null`, never `undefined` mixed in: store `get(id)`
  returns `Session | null`; `activeTab` computed returns `OpenTab | null`.
- Store accessors accept `string | null | undefined` ids so callers can pass
  computed values directly: `sessionStore.get(workspace.activeSessionId)`.
- Use `?? null` / `?? ''` fallbacks at boundaries rather than non-null `!`.
  The `!` assertion is not used in this codebase — avoid introducing it.
- Partial updates are typed `Partial<Message>` / `Partial<ToolCall>`
  (`patchMessage(sessionId, messageId, patch: Partial<Message>)`).

---

## Constants with literal types

Module-level constant arrays that feed UI options are typed so their values
stay literal:

```ts
export const MODELS = [...] as const
export const EXECUTION_MODES: { value: ExecutionMode; label: string }[] = [...]
```

---

## Verification

Before finishing any change: `pnpm build` (runs `vue-tsc --noEmit`). There is
no separate lint/typecheck script; do not weaken tsconfig flags to make code
pass — fix the code.
