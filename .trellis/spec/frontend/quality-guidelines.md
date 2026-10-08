# Quality Guidelines

> Verification tooling and code-quality rules. Documents the **current reality**
> of this repo, which is intentionally lean.

---

## Tooling status (reality check)

| Concern | Status |
|---|---|
| Package manager | **pnpm** (`pnpm-lock.yaml`, `pnpm-workspace.yaml` present). Do not commit `package-lock.json` / `yarn.lock`. |
| Type check | `vue-tsc --noEmit`, runs as part of `pnpm build` |
| Lint (ESLint) | **Not configured.** No `eslint.config.js`. Do not import lint-fix habits that assume ESLint; do not add ESLint as a side effect of another task. |
| Format (Prettier) | **Not configured.** No `.prettierrc` / `.editorconfig`. Formatting follows the existing file style (see below). |
| Tests | **None.** No test runner, no `*.spec.ts` / `*.test.ts`. Verification is manual in the running app. |
| CI | None configured. |

---

## Verification commands

```bash
pnpm build        # vue-tsc --noEmit && vite build — the primary quality gate
pnpm dev          # Vite dev server on :1420 (strictPort), browser-only
pnpm dev:tauri    # full Tauri dev via tauri-dev.bat (desktop shell)
```

- Every change must pass `pnpm build` (type errors are build failures).
- Because there are no tests, exercise the affected flow in the app
  (`pnpm dev:tauri`) before reporting completion; state what was verified.

---

## Formatting conventions (inferred from the codebase — match it)

- Indent: 2 spaces (TS/Vue/CSS).
- Quotes: single quotes in TS; double quotes tolerated in CSS/HTML attributes.
- Semicolons: **no trailing semicolons** in TS statements (see stores, utils,
  composables), except where required (e.g. lines starting with `(` or `[`).
- Trailing commas in multiline literals/params: yes.
- Vue SFC block order: `<script setup>` → `<template>` → `<style scoped>` (rare).
- Keep lines readable; existing code wraps around ~100 columns.
- Files end with a newline; repo is Windows (`CRLF` appears in some files —
  `.gitattributes` governs; don't hand-normalize line endings in diffs).

---

## Code-quality rules

1. **Chinese comments, Chinese UI strings.** Top-of-file purpose comment for
   every non-trivial module; `/** */` JSDoc on exported functions and
   non-obvious type members. Comment *why*, not *what*.
2. **No dead code / unused imports** — `noUnusedLocals` + `noUnusedParameters`
   make these build errors. Remove commented-out blocks instead of keeping them.
3. **No `any`, no non-null `!`** (see `type-safety.md`).
4. **No `console.log` left in committed code.** Temporary debugging output must
   be removed before finishing.
5. **Don't introduce new global state.** App-wide state goes into an existing
   Pinia store or a new store with a clear boundary; module-level mutable state
   is only allowed for the documented cross-mount patterns in composables
   (`controllers`, `positions` maps).
6. **Keep the mock seams.** `conversationService`, `stores/project.ts`, and
   `utils/appData.ts` are deliberate placeholders for future Tauri backends.
   Extend behind those seams; don't scatter `invoke()` calls through components.
7. **Guard re-entry on async user actions** (pattern: `if (workspace.isGenerating(id)) return`).
8. **Clean up listeners/timers** — `onBeforeUnmount` removes window listeners
   (see drag handling in `CodingWorkspace.vue`, `App.vue`); abort in-flight
   streams on stop/unmount via the session's `AbortController`.
9. **Frameless-window contract:** clickable regions inside `[data-drag-region]`
   must be excluded with `data-no-drag` or standard interactive tags, or they
   become undraggable/unclickable. Verify new top-bar UI in the Tauri shell.
10. **No new dependencies without need.** Current set is deliberate: naive-ui
    (widgets), lucide-vue-next (icons), Tailwind v4 (styling), monaco (editor),
    xterm (terminal), splitpanes (layout), marked (markdown). Prefer these over
    adding libraries.

---

## Forbidden patterns

| Pattern | Why / what to do instead |
|---|---|
| Options API / `export default { }` components | Project is 100% `<script setup lang="ts">` |
| Runtime props declaration (`props: { ... }`) | Use type-only `defineProps<{ ... }>()` |
| Pinia options stores (`state/getters/actions`) | Use setup stores returning refs/computed/functions |
| Direct fetch/axios to "backends" from components | Go through `services/` + composable |
| `v-html` with untrusted content | Currently only `MarkdownContent.vue` renders `marked` output of local mock content; keep it that way |
| Weakening tsconfig strictness to pass build | Fix the code |
| Hardcoded entity IDs | Use `createId(prefix)` from `utils/id.ts` |
| Inline date/number formatting | Use `utils/time.ts` (`formatRelativeTime`, `formatTokens`, ...) |
