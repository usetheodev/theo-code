---
paths:
  - "apps/theo-ui/**/*.{ts,tsx,css}"
  - "apps/theo-desktop/src/**/*.{ts,tsx}"
---

# Frontend Conventions

## Stack
- React 18 + TypeScript (strict mode)
- Tailwind CSS for styling
- Radix UI for accessible primitives
- Tauri v2 for desktop shell

## Components
- Functional components only. No class components.
- Props interfaces named `{ComponentName}Props`.
- Colocate styles, types, and tests with components.
- Use Radix UI primitives for interactive elements (Dialog, DropdownMenu, Tooltip, etc).

## State
- Local state with `useState` / `useReducer`.
- Server state with React Query or SWR if needed.
- No global state unless absolutely necessary.

## Accessibility
- All interactive elements must be keyboard navigable.
- Use semantic HTML: `<button>` not `<div onClick>`.
- ARIA labels on icon-only buttons.
- Radix UI handles most a11y — don't override it.

## Code Wiki UI
- Wiki pages rendered as markdown with syntax highlighting.
- Navigation follows Obsidian patterns: sidebar tree + search.
- Internal links between wiki pages use `[[module-name]]` syntax.
- Graph visualization for dependency maps.
