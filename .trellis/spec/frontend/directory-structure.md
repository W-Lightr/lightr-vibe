# Directory Structure

> How frontend code is organized in this project (Vue 3 + Tauri 2 + Vite).

---

## Overview

The app is a Tauri desktop AI coding workbench ("Lightr-vibe"). The frontend
lives in `src/` (Vue 3 + TypeScript); the Rust shell lives in `src-tauri/`
and is currently minimal (a placeholder `greet` command only — all app data
is mocked in the frontend for now).

---

## Directory Layout

```
src/
├── main.ts                 # App bootstrap: Monaco workers, Pinia, Router
├── App.vue                 # Root: frameless-window drag, NConfigProvider/NMessageProvider
├── assets/
│   └── main.css            # Tailwind v4 entry (@import "tailwindcss") + global tweaks
├── composables/            # useXxx() functions (e.g. useConversation, useConversationScroll)
├── layouts/
│   └── WorkspaceLayout.vue # Route layout wrapper (thin <router-view/>)
├── router/
│   └── index.ts            # Route table
├── services/
│   └── conversationService.ts  # Backend-facing (currently mock) conversation API
├── stores/                 # Pinia setup stores: project.ts, session.ts, workspace.ts
├── types/
│   └── session.ts          # Core domain model types (Session, Message, OpenTab, ...)
├── utils/                  # Pure helpers: appData.ts, file.ts, id.ts, time.ts
└── views/
    ├── HomeView.vue        # Standalone page
    └── coding/
        ├── CodingWorkspace.vue   # Feature root page
        └── components/           # Feature-local components (SessionTabs, TopBar, ...)
```

---

## Module Organization

- **Feature-scoped components** live next to their feature page:
  `src/views/<feature>/components/`. Everything the coding workspace needs is
  in `src/views/coding/components/` (e.g. `SessionItem.vue`, `MessageComposer.vue`).
- **Cross-feature pages** stay directly in `src/views/` (`HomeView.vue`).
- A component used by only one feature must NOT be promoted to a global
  `components/` dir — there is no global components directory in this project.
- **Shared pure logic** goes to `src/utils/` (no Vue imports): ID generation
  (`id.ts`), time formatting (`time.ts`), file-type mapping (`file.ts`),
  app data dir resolution (`appData.ts`).
- **Logic that touches Pinia stores** goes to `src/composables/` (`useConversation.ts`).
- **Backend/async data access** goes to `src/services/` — components never
  call mock APIs directly, they go through `conversationService` via a
  composable or store.

### Feature-local state, and the `views/<feature>/*.ts` slot

Task board (10-17-task-board) added a second full feature next to `coding`:

```
src/views/tasks/
├── TasksView.vue          # page
├── boardColumns.ts        # feature-local PURE logic (status → column mapping)
└── components/            # TaskCard, TaskColumn, dialogs, useTaskDrag.ts
src/types/task.ts          # domain model (shared by service, store, views)
src/services/taskService.ts + taskMock.ts
src/stores/task.ts
```

- A feature may keep **pure, feature-only** modules directly beside its page
  (`boardColumns.ts`). They are not promoted to `src/utils/` because nothing
  outside the feature imports them — `utils/` stays cross-feature only.
- A feature-scoped composable lives in `views/<feature>/components/` next to
  the components that use it (`useTaskDrag.ts`), not in the global
  `src/composables/`, which is reserved for cross-feature state (`useConversation`).
- Domain types still go to `src/types/` even when one feature owns them —
  the service/store boundary must be able to import them without pulling in a
  view module.


---

## Naming

- Files: `PascalCase.vue` for components/pages, `camelCase.ts` for modules.
- Composables: `useXxx.ts` exporting `useXxx()`.
- Stores: `<domain>.ts` exporting `use<Domain>Store`.
- No path aliases are configured (`tsconfig.json` has no `paths`). All imports
  are **relative** (`../../../stores/workspace`). Keep it that way.

---

## Routing

`src/router/index.ts`: top-level pages import eagerly (`HomeView`); nested
feature routes lazy-load with `() => import(...)` behind a layout:

```ts
{
  path: '/coding',
  component: () => import('../layouts/WorkspaceLayout.vue'),
  children: [
    { path: '', name: 'coding', component: () => import('../views/coding/CodingWorkspace.vue') },
  ],
}
```

---

## Tauri boundary

- Rust side (`src-tauri/src/lib.rs`) is scaffold-only. Frontend features that
  will eventually need Rust (fs read/write, shell) are mocked behind
  `services/` or `stores/project.ts` — keep that seam so swapping in
  `invoke()` later touches one layer only.
- Tauri APIs are imported **dynamically with fallback** when the code must
  also run in a plain browser dev server — see `src/utils/appData.ts`
  (`await import('@tauri-apps/api/path')` inside try/catch, placeholder
  `~/.lightr-vibe` on failure).
