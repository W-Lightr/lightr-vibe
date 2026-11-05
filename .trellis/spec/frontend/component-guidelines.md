# Component Guidelines

> Vue SFC conventions used across this codebase.

---

## SFC shape

Every component is a single-file component using `<script setup lang="ts">`,
ordered **script → template → (optional) style**:

```vue
<script setup lang="ts">
import { NPopconfirm } from 'naive-ui'
import { Pencil, Trash2 } from 'lucide-vue-next'
import type { Session } from '../../../types/session'
import { formatRelativeTime } from '../../../utils/time'
import { useWorkspaceStore } from '../../../stores/workspace'

const props = defineProps<{ ... }>()
const emit = defineEmits<{ ... }>()

const workspace = useWorkspaceStore()
</script>

<template> ... </template>
```

- No `export default`, no Options API, no separate `.ts` files for simple components.
- `<script setup>` block organization: imports → props/emits → store/composable
  instances → local refs/computed → functions → lifecycle hooks.

---

## Props

Type-only `defineProps` with an inline type literal (no `PropType`, no runtime
declaration). Example from `src/views/coding/components/SessionItem.vue`:

```ts
const props = defineProps<{
  session: Session
  active: boolean
  renaming: boolean
  renameDraft: string
}>()
```

- Props carry **domain types** from `src/types/session.ts`, not ad-hoc shapes.
- `withDefaults` is not used in this codebase; optional props use `?:` in the
  type literal (e.g. `defineProps<{ tab: OpenTab }>()`).
- Destructuring props is avoided; access via `props.xxx` (keeps reactivity obvious).

## Emits

Type-only `defineEmits` with the tuple syntax. Example (`SessionItem.vue`):

```ts
const emit = defineEmits<{
  select: []
  rename: []
  'commit-rename': []
  remove: []
  'update:renameDraft': [value: string]
}>()
```

- Multi-word event names are **kebab-case** (`commit-rename`, `cancel-rename`).
- `v-model` on a prop is expressed as `'update:xxx'` emit + parent-side binding.
- Leaf components emit intent; they don't mutate parent state directly.

---

## State access pattern

- Components read/mutate shared state **through Pinia setup stores**
  (`useWorkspaceStore()` etc.), including direct store-property writes in
  templates when that is the simplest form:
  `@click="workspace.activeTabId = tab.id"` (see `SessionTabs.vue`).
- Cross-store orchestration and async flows belong in composables
  (`useConversation`) or stores, not in components.
- Component-local UI state (drafts, element refs, panel toggles that don't
  outlive the component) stays in local `ref()`s — e.g. `draft`, `textareaEl`
  in `MessageComposer.vue`.

---

## Styling

- **Tailwind CSS v4 utility classes in templates are the default.** Colors use
  the neutral/slate/blue palettes (`text-neutral-600`, `bg-neutral-200/70`,
  `bg-blue-500`); arbitrary values are allowed (`text-[13px]`, `z-[1]`).
- Conditional classes via `:class` ternaries or arrays (see `SessionTabs.vue`).
- `<style scoped>` plain CSS is the exception, used only where utilities are
  awkward: `TopBar.vue` (drag-region buttons), `MarkdownContent.vue`
  (rendered-markdown child selectors). Global CSS additions go to
  `src/assets/main.css` (e.g. splitpanes splitter theming).
- Interactive elements inside the frameless-window drag region must carry
  `data-no-drag` (drag logic in `App.vue` excludes `[data-no-drag]` and
  standard interactive selectors).
- Hide scrollbars with the utility pattern used in `SessionTabs.vue`:
  `[scrollbar-width:none] [&::-webkit-scrollbar]:hidden`.

---

## UI libraries

- **naive-ui**: imported **per component** (`import { NPopconfirm } from 'naive-ui'`),
  never globally registered. App-level providers (`NConfigProvider`,
  `NMessageProvider`) live in `App.vue` only. Used for popconfirm/select-style
  widgets; most chrome is hand-built with Tailwind.
- **lucide-vue-next**: icon source, named imports per icon
  (`import { Pencil, Trash2 } from 'lucide-vue-next'`).
- **@xterm/xterm** + addon-fit: terminal (`TerminalPanel.vue`); its CSS is
  imported by `TerminalPanel.vue` itself so it loads with the terminal chunk.
- **monaco-editor** with `monaco-editor-vue`: file editing (`FileEditorView.vue`)
  and diff view (`DiffViewer.vue`). Monaco workers are configured in
  `src/views/coding/monacoSetup.ts` via Vite `?worker` imports; every Monaco
  consumer imports it on its first line, before `monaco-editor-vue`. Do not
  configure workers in `main.ts` or per component.
  Note `optimizeDeps.exclude: ['monaco-editor']` in `vite.config.ts`.
- Editor, diff view and terminal are loaded through `defineAsyncComponent` in
  `CodingWorkspace.vue` / `RightPanel.vue` (see `lazy-loading.md`); never
  re-add static imports.
- **splitpanes**: pane layout in the workspace.
- **marked**: markdown → HTML for `MarkdownContent.vue` (rendered via `v-html`
  on trusted local content only).

---

## Comments

- File/module purpose comment at the top of scripts (Chinese), e.g.
  `// 会话生成控制：发送消息、流式更新、停止生成。`
- Inline `<!-- ... -->` template comments explain layout intent (Chrome-style
  tabs, drag regions). Match this density: comment *why*, not *what*.
- User-facing strings are **Chinese** (labels, buttons, placeholders).
