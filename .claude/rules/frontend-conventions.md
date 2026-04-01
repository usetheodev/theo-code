---
paths:
  - "theo-code/apps/theo-ui/**/*.ts"
  - "theo-code/apps/theo-ui/**/*.tsx"
  - "theo-code/apps/theo-desktop/ui/**/*.ts"
  - "theo-code/apps/theo-desktop/ui/**/*.tsx"
---

# Convenções Frontend

- React 18 com componentes funcionais e hooks
- TypeScript strict — sem `any` exceto em tipos de terceiros
- Styling: Tailwind CSS + class-variance-authority para variantes
- Componentes UI base: Radix UI primitives em `components/ui/`
- Animações: Framer Motion
- Ícones: Lucide React
- Bridge desktop: `@tauri-apps/api` para comunicação com backend Rust
- Estrutura: features/ para domínios, components/ui/ para primitivas
- Usar `cn()` (clsx + tailwind-merge) para composição de classes
- Eventos do agent via `useAgentEvents` hook customizado
