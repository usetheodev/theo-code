# theo-ui — Vite/React frontend

Frontend bundle for the Theo Desktop shell. **TypeScript + React 18 + Tailwind +
Radix UI + Tauri v2.** Lives outside the Rust Cargo workspace; consumed by
`apps/theo-desktop` (Tauri) at build time.

## Why this is here, not in `crates/`

`apps/` is the conventional location for runnable surfaces in this repo. The
two binary apps (`theo-cli`, `theo-desktop`, `theo-marklive`) sit alongside this
directory; this one is the React side of `theo-desktop`. Tauri loads the
built bundle at runtime — so the build pipeline is:

```
apps/theo-ui/         (this directory — npm run build → dist/)
   └─ apps/theo-desktop/  (Cargo crate — bundles dist/ into the Tauri shell)
        └─ theo-code-desktop binary
```

## Requirements

- **Node.js** ≥ 18
- **npm** ≥ 9 (or compatible package manager)

## Install

```bash
cd apps/theo-ui
npm install
```

## Build

```bash
npm run build         # tsc && vite build → emits dist/
```

## Dev server

```bash
npm run dev           # http://localhost:5173 by default
```

When developing the desktop shell, run `cargo tauri dev` from the workspace
root instead — it handles the Vite dev server lifecycle.

## Test

```bash
npm test                          # vitest run
npm run audit:circ                # madge — circular dep audit
npm run audit:licenses            # license-checker summary
npm run audit:mutation            # Stryker mutation testing
```

## Stack

- **React 18** + **TypeScript 5**
- **Vite 6** for build/dev
- **Tailwind 3** + **tailwindcss-animate** for styling
- **Radix UI** primitives (dialog, separator, tabs, tooltip)
- **lucide-react** icons + **framer-motion** for transitions
- **react-router 6** for in-app navigation
- **@tauri-apps/api 2** + **@tauri-apps/plugin-dialog** for desktop integration

## Layout

```
src/
├── main.tsx                Entry point
├── main-dashboard.tsx      Top-level layout
├── app/                    Route-level pages
├── features/               Feature modules
├── components/             Reusable UI primitives
├── hooks/                  React hooks
├── lib/                    Pure helpers (no React)
├── types.ts                Shared types
└── styles.css              Tailwind entry
```

## Conventions

- TypeScript strict mode; type errors block `npm run build`.
- Tailwind class-merging via `tailwind-merge` + `clsx` (`cn()` helper in `lib/`).
- Radix UI primitives wrapped before use — components in `components/`
  consume the wrapped variants, never Radix directly.

## Not in scope here

- The Rust workspace (`cargo build` / `cargo test`) — see `/CLAUDE.md`.
- The Python benchmark harness (`apps/theo-benchmark/`) — see its own README.
- The Tauri shell (`apps/theo-desktop/`) — Cargo crate, depends on the built
  `dist/` from this directory.
