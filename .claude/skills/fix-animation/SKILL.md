---
name: fix-animation
description: Use when implementing or fixing animations in the Theo Code desktop app. Covers Framer Motion, CSS transitions, hover effects, tooltips, loading states, agent streaming feedback, and performance. Applies to theo-ui components.
---

# Fix Animation

Animation patterns for the Theo Code desktop app (React + Framer Motion + Tailwind).

## Context

Stack: React 18, Framer Motion 12, Tailwind CSS 3, Radix UI
App type: Desktop (Tauri v2) — no mobile touch concerns, but keyboard-heavy

## Critical Rules

### Performance
- Only animate `transform` and `opacity` — never `width`, `height`, `top`, `left`
- Use `will-change: transform` only when needed and remove after animation
- Keep animations under 300ms for UI feedback
- No `requestAnimationFrame` loops without stop condition

### Framer Motion Patterns
```tsx
// Enter/exit with AnimatePresence
<AnimatePresence mode="wait">
  {isVisible && (
    <motion.div
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -4 }}
      transition={{ duration: 0.15, ease: [0.32, 0.72, 0, 1] }}
    />
  )}
</AnimatePresence>

// Stagger children
<motion.div variants={container} initial="hidden" animate="show">
  {items.map(item => (
    <motion.div key={item.id} variants={child} />
  ))}
</motion.div>

// Layout animation for reordering
<motion.div layout layoutId={item.id} />
```

### Agent-Specific Animations
- **Streaming text**: Character-by-character with 10ms delay, not word-by-word
- **Phase transitions** (LOCATE→EDIT→VERIFY→DONE): Slide + fade, 200ms
- **Tool call indicators**: Pulse animation while executing, check/x on complete
- **Decision badges** (APPROVE/REJECT): Scale from 0.95 with color transition
- **Loading skeleton**: Shimmer effect for content loading, not spinner

### Easing
- Enter/exit: `[0.32, 0.72, 0, 1]` (fast start, smooth land)
- On-screen movement: `[0.4, 0, 0.2, 1]` (ease-in-out)
- Spring for interactive: `{ type: "spring", stiffness: 400, damping: 30 }`

### Reduced Motion
```tsx
const prefersReduced = useReducedMotion();
// Swap slide for instant, keep opacity fade (shorter)
```

### Anti-Patterns
- No `scale(0)` start — use `scale(0.95)` minimum
- No animation on keyboard navigation (tab, arrow keys)
- No tooltip animation after first tooltip is already open
- No spinner for operations under 200ms

Argument: $ARGUMENTS
