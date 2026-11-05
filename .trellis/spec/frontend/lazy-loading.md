# Lazy Loading Feature Entries

> How to add a new feature entry without dragging heavy dependencies into the
> first paint. Derived from the coding-entry optimization
> (`src/views/coding/lazyWorkspace.ts`, `src/views/coding/monacoSetup.ts`).

---

## Why

`/coding` originally loaded Monaco (~2.7 MB) and xterm (~0.3 MB) as static
dependencies of the workspace chunk, even though neither renders on first paint.
Every new feature (e.g. writing, canvas) would repeat that mistake if entries
are not structured deliberately.

---

## Rules

### 1. Feature pages are route-level lazy imports

Top-level pages that a user navigates to must be route-level lazy imports
(`() => import(...)`) in `src/router/index.ts`. Home and shared layouts must not
statically import a feature page.

```ts
// src/router/index.ts
{
  path: '/coding',
  component: () => import('../layouts/WorkspaceLayout.vue'),
  children: [
    {
      path: '',
      name: 'coding',
      component: () => import('../views/coding/CodingWorkspace.vue').then((m) => m.default),
    },
  ],
}
```

Always unwrap the SFC default export with `.then((m) => m.default)`. Do **not** "simplify"
this back to a bare `import('../views/.../XxxView.vue')`: returning the raw module namespace
makes the bundler emit a namespace-wrapper export whose shared helper is currently chunked
with Monaco, which creates a static edge route chunk → `editor.api` (~2.7 MB) and silently
undoes this optimization. The same applies to any prefetch import of the page
(`void import('...').then((m) => m.default)`).

Verify a change like this with the built output, not by eye:

```bash
# 路由 chunk 的静态 import 里不应出现 editor.api / dist-*.js（__vite__mapDeps 里的动态依赖不算）
grep -o 'from"\./[^"]*"' dist/assets/CodingWorkspace-*.js
```

### 2. Heavy dependencies only live inside async sub-modules of the feature

Monaco, xterm, canvas renderers, PDF viewers and similar multi-MB packages are
forbidden in:

- `src/views/HomeView.vue`
- `src/layouts/*` (shared layouts)
- the feature page itself (`CodingWorkspace.vue` / future `WritingView.vue`)
- the feature's statically imported child components

They may only appear in components that the feature loads through
`defineAsyncComponent`, and every dynamic `import()` should be declared once in
the feature's `lazyWorkspace.ts`-style module (see `src/views/coding/lazyWorkspace.ts`).

```ts
const FileEditorView = defineAsyncComponent({
  loader: loadFileEditor,
  loadingComponent: EditorLoading,
  errorComponent: AsyncPanelError,
  delay: 0,
  timeout: 30_000,
})
```

### 3. Every async component needs loading and error fallbacks

- `loadingComponent`: visible immediately (`delay: 0`), same background color as
  the final panel to avoid a white flash.
- `errorComponent`: shows the failure and a retry button; it receives the
  injected `error` / `retry` props.
- Do **not** wrap the whole page in `<Suspense>` — a slow chunk must never block
  the entire workspace.

### 4. Prefetch only on idle or user intent

- After the first frame, prefetch the editor chunk at idle time:
  `scheduleIdle(prefetchFileEditor, 3000)` (uses `requestIdleCallback` with a
  `setTimeout` fallback).
- Prefetch other chunks on `pointerenter` / `mouseenter` / `focus` of the
  triggering control (terminal button, file tree, home feature card).
- Prefetch the feature page itself only through its route form (`.then((m) => m.default)`),
  never by importing the raw module namespace.
- Never prefetch a heavy chunk synchronously from the home page or shared
  layout.

### 5. Any module that uses Monaco must import `monacoSetup` first

`src/views/coding/monacoSetup.ts` owns the five `?worker` imports and the
`MonacoEnvironment` assignment. A module that renders Monaco (currently
`FileEditorView.vue` and `DiffViewer.vue`) must put

```ts
import './monacoSetup' // or '../monacoSetup'
```

**before** importing `monaco-editor-vue` / `monaco-editor`. Static import order
is evaluation order in ESM, and Monaco reads `globalThis.MonacoEnvironment`
lazily when it spawns a worker, so the factory is in place before any editor is
created. Do not duplicate the worker configuration elsewhere.

The xterm stylesheet (`@xterm/xterm/css/xterm.css`) is imported by
`TerminalPanel.vue`, so it loads together with the terminal chunk. Do not move
it back to `main.ts`.

---

## Checklist for a new feature entry

- [ ] Route registered with `() => import(...).then((m) => m.default)`, not a static import.
- [ ] Route chunk's static imports contain no `editor.api` / Monaco chunk (check `dist/assets`).
- [ ] Feature page statically imports only shell/session components.
- [ ] Heavy dependencies are in `defineAsyncComponent` sub-modules.
- [ ] Dynamic imports are declared once, in a feature-local `lazyWorkspace.ts`-style module.
- [ ] Each async component has `loadingComponent` + `errorComponent` (+ `delay: 0`).
- [ ] Prefetch happens at idle or on hover/focus only.
- [ ] Monaco-based modules import `monacoSetup` before Monaco.
- [ ] `pnpm build` passes and `dist/index.html` does not preload the heavy chunk.
