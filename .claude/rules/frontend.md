---
paths:
  - "apps/theo-ui/**/*.{ts,tsx,css}"
---

# Frontend Conventions

## Stack
- React 18 + TypeScript (strict mode)
- Tailwind CSS for styling
- Radix UI for accessible primitives
- Framer Motion for transitions
- Tauri v2 integration via `@tauri-apps/api` and invoke-based commands

## Components
- Functional components only. No class components.
- Props interfaces named `{ComponentName}Props` when exported or reused.
- Colocate styles, types, and tests with components.
- Wrap and reuse local UI primitives instead of importing Radix ad hoc across feature code.

## State
- Local state with `useState` / `useReducer`.
- Tauri/backend data is typically fetched through `invoke(...)`, not a web API client layer.
- Do not introduce a global state library unless multiple feature modules demonstrably need shared mutable state.

## Accessibility
- All interactive elements must be keyboard navigable.
- Use semantic HTML: `<button>` not `<div onClick>`.
- ARIA labels on icon-only buttons.
- Radix UI handles most a11y — don't override it.

## Project-Specific Notes
- Keep `npm run build` green; type errors block the build.
- Prefer the existing route/feature split under `src/app` and `src/features`.
- The UI serves desktop/dashboard flows; do not assume generic SPA backend patterns that the repo does not use.
