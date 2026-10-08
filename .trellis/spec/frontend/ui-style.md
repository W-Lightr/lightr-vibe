# UI Style

> Visual design language of Lightr-vibe, extracted from real Tailwind class
> frequency across all `.vue` files. **Light, minimal desktop-tool aesthetic:
> white base + neutral gray scale + a single blue accent, Chrome-style tabs,
> high information density, small type, almost no shadows.** Think Cursor /
> Linear (light) desktop sensibility.

---

## Color system

The app is **light-only** (no dark mode; the sole dark surface is the xterm
terminal). Use the Tailwind `neutral` palette — not `gray`/`zinc`/`stone`.

| Role | Classes | Notes |
|---|---|---|
| App base | `bg-white` + `text-neutral-800` | Set once on the root wrapper in `App.vue` |
| Panel / secondary surface | `bg-neutral-100`, `bg-neutral-50` | Sidebars, hover backgrounds, sub-panels |
| Selected / active surface | `bg-neutral-200/70`, `bg-neutral-200` | List selection, tab hover |
| **Accent (only one)** | `bg-blue-500`, `text-blue-500`, `border-blue-400` | Status dots, generating pulse, focus borders, splitter hover (`#3b82f6`) |
| Body text | `text-neutral-600` | Default content |
| Secondary text | `text-neutral-500` | Labels, timestamps |
| Muted / placeholder | `text-neutral-400` (most used text class, 42×) | Icons at rest, hints, inactive tab text |
| Headings / emphasis | `text-neutral-800`, `text-neutral-900` | Titles, active tab text |
| Divider / border | `border-neutral-200` (dominant, 19×), `border-neutral-300` for inputs | 1px hairlines |
| Danger | `red-500` / `red-600` / `red-50` | Window close hover (`bg-red-500` + white icon), destructive actions |
| Success | `green-600` | Sparse |
| Warning | `amber-400` | Sparse |

Rules:

- **No gradients, no multi-hue decoration.** Blue is the only non-gray UI hue;
  red/green/amber appear only with semantic meaning.
- Translucent neutrals are an established idiom for hover/active layers:
  `bg-neutral-200/50`, `/60`, `/70` — reuse instead of inventing new steps.
- Depth comes from **gray-step layering + 1px borders**, not shadows.

---

## Typography

Desktop-tool density: type runs small.

| Use | Class | Frequency evidence |
|---|---|---|
| Dense labels, badges, meta | `text-[11px]` | 23× — co-primary size |
| Default small text | `text-xs` | 22× |
| Tabs, list items, primary UI text | `text-[13px]` (rarely `text-[13.5px]`) | 9× |
| Descriptions, inputs | `text-sm` | 7× |
| Micro labels | `text-[10px]` | 3× |
| Page hero only | `text-3xl font-semibold tracking-wide` | 1× (`HomeView.vue` h1) |

- Arbitrary px sizes (`text-[13px]`) are accepted and common — match the
  surrounding component's size rather than snapping to Tailwind steps.
- Weights: default normal; `font-medium` (10×) marks active/important items;
  `font-semibold` reserved for page titles; `font-mono` for code/paths.
- Truncation idiom for tab/list labels: `min-w-0 flex-1 truncate`.

---

## Shape, spacing, icons

- **Radius scale**: `rounded-md` is the house default (31×) — buttons, inputs,
  list rows, panels. `rounded-full` for dots/avatars; `rounded-xl` for cards
  (Home feature cards); `rounded-t-xl` for Chrome tabs; `rounded-sm`/`rounded`
  for micro elements.
- **Shadows**: essentially none. Whole codebase: `shadow-sm`×2, `shadow-md`×1
  (Home cards, with `hover:shadow-md`). Do not add shadow-based elevation to
  workspace UI.
- **Spacing**: tight. `gap-1.5` / `gap-1` / `gap-0.5` dominate; `gap-2`–`gap-3`
  for grouped content; large gaps (`gap-6`+) only on the Home page.
- **Icons**: `lucide-vue-next`, sized `h-4 w-4` (standard), `h-3.5 w-3.5`
  (dense areas), `h-3 w-3` (minimum). Icon color follows adjacent text
  (`text-neutral-400/500` at rest).
- **Hit targets**: icon buttons are `h-8 w-8` (window controls) or `h-7 w-7`,
  always with `cursor-pointer` and a hover background.

---

## Layout & window chrome

- **Frameless custom window**: drag via `[data-drag-region]`, interactive
  elements excluded via `[data-no-drag]` (logic in `App.vue`). Custom
  minimize/maximize/close buttons top-right; **close hover = `bg-red-500` +
  `text-white`** (Windows convention).
- **Chrome-style tab strip** (`SessionTabs.vue`): gray strip background; active
  tab is white with `rounded-t-xl` and "cuts through" the 1px bottom divider;
  inactive tabs blend into the strip. Horizontal overflow scrolls with hidden
  scrollbars: `[scrollbar-width:none] [&::-webkit-scrollbar]:hidden`.
- **Three-pane workspace** with custom drag splitters (pointer events in
  `CodingWorkspace.vue`); left 200–480px, right 260–560px. Splitpanes splitter
  themed in `main.css`: `#e5e5e5` line, hover `#3b82f6`.
- Global CSS lives in `src/assets/main.css` (Tailwind v4 entry + splitpanes
  theming only).

---

## Motion & states

Motion is deliberately minimal:

- Transitions: `transition-colors` (hover), `transition-transform` (press/rotate),
  occasionally `transition-opacity` / `transition-all`. ~10 usages total —
  no elaborate animation.
- **Generating / running** = `animate-pulse` blue dot (`h-1.5 w-1.5 rounded-full
  bg-blue-500`) or `animate-spin` `LoaderCircle` in `text-blue-500`.
- **Streaming text** = pulsing caret bar: `h-4 w-1.5 animate-pulse bg-neutral-400`.
- **Disabled** = `opacity-40 cursor-not-allowed` (not color swaps).
- **Hover-revealed controls** (tab close button etc.): `opacity-0` →
  `group-hover:opacity-100` (keep `focus-visible:opacity-100` for a11y), with
  the slot always occupying space to avoid layout jump.
- Hover feedback is a background step (`hover:bg-neutral-100` / `/200` family),
  not border or shadow changes — exception: Home feature cards
  (`hover:border-blue-300 hover:shadow-md`).

---

## Component-library styling boundary

- naive-ui is used **unskinned** (`NConfigProvider` with no `themeOverrides`)
  and only for behavior-heavy widgets (NPopconfirm, NSelect). All visual chrome
  is hand-built with Tailwind — keep it that way; don't restyle via
  themeOverrides, and don't reach for naive-ui where a few utility classes do.
- Monaco editor: light `vs` theme. xterm: `{ background: '#171717',
  foreground: '#e5e5e5' }` — the only dark surface in the app.

---

## Language

All user-facing strings are **Chinese** (buttons, labels, placeholders,
`title` tooltips). Follow this for any new UI copy.

---

## Quick checklist for new UI

1. White/neutral surface, `border-neutral-200` hairline if separation needed —
   no shadow.
2. `rounded-md`, small type (`text-xs`–`text-[13px]`), tight gaps.
3. Grays for hierarchy; blue only for active/generating/focus; red only for
   destructive/close.
4. lucide icon at `h-4 w-4` or smaller, `text-neutral-400/500`.
5. Hover = background step; disabled = `opacity-40`; async = pulse/spin.
6. Inside the drag region, add `data-no-drag` to anything interactive.
7. Chinese copy.
