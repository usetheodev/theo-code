---
name: fix-accessibility
description: Use when adding or changing buttons, inputs, dialogs, tabs, menus, forms, focus states, or icon-only controls in the Theo Code desktop app. Covers WCAG compliance for Radix UI + Tailwind components.
---

# Fix Accessibility

Accessibility rules for the Theo Code desktop app (React + Radix UI + Tailwind).

## Priority Order

1. **Accessible names** (critical) — every interactive control needs one
2. **Keyboard access** (critical) — all interactive elements reachable by Tab
3. **Focus and dialogs** (critical) — modals trap focus, restore on close
4. **Semantics** (high) — native elements over role-based hacks
5. **Forms and errors** (high) — errors linked to fields via aria-describedby

## Quick Fixes

```tsx
// Icon-only button: add aria-label
<Button size="icon" aria-label="Close">
  <X aria-hidden="true" />
</Button>

// Form error: link with aria-describedby
<Input id="email" aria-describedby="email-err" aria-invalid={!!error} />
<p id="email-err" role="alert">{error}</p>

// Dialog: always needs a title (visually hidden if needed)
<Dialog>
  <DialogTitle className="sr-only">Agent Settings</DialogTitle>
  ...
</Dialog>

// Loading state: announce to screen readers
<div aria-busy={isLoading} aria-live="polite">
  {isLoading ? <Skeleton /> : content}
</div>

// Agent status: announce phase changes
<div role="status" aria-live="polite">
  Phase: {currentPhase}
</div>
```

## Radix UI Specifics

- Radix handles most ARIA automatically — don't add redundant attributes
- `DialogTitle` is REQUIRED even if visually hidden
- `TabsTrigger` must be inside `TabsList`
- Use `asChild` for custom triggers, not wrapper divs

## Theo Code Specific

- Agent view tabs (Agent, Plan, Tests, Review, Security) must be keyboard navigable
- Real-time streaming text must have `aria-live="polite"` (not "assertive")
- Decision badges (APPROVE/REJECT) must have text alternative, not just color
- Tool call list must be navigable with arrow keys

## Process

1. Read the specified file(s)
2. Check against rules above (priority order)
3. Report: violation, why it matters (1 sentence), concrete fix
4. Prefer minimal changes — don't refactor unrelated code
5. Prefer native HTML before adding ARIA

Argument: $ARGUMENTS
