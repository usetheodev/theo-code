# Meeting — 2026-04-06 (Unificar EXCLUDED_DIRS + gitignore fallback)

## Proposta
Unificar EXCLUDED_DIRS em theo-domain, usar .add_ignore() para .gitignore fallback, suportar .theoignore.

## Participantes
- **governance** — APPROVE com ajuste: usar .add_ignore() do crate ignore, não reimplementar parser

## Veredito
**APPROVED**

## Escopo Aprovado
- `crates/theo-domain/src/graph_context.rs` (constante EXCLUDED_DIRS)
- `crates/theo-application/src/use_cases/extraction.rs` (importar constante + .add_ignore + .theoignore)
- `crates/theo-application/src/use_cases/graph_context_service.rs` (importar constante + .add_ignore + .theoignore)

## Condições
1. Constante em theo-domain/src/graph_context.rs (conhecimento de domínio)
2. .add_ignore(project_dir.join(".gitignore")) — fallback quando .git/ ausente
3. .add_custom_ignore_filename(".theoignore") — extensibilidade
4. NÃO reimplementar parser de .gitignore
5. Remover EXCLUDED_DIRS duplicado dos dois arquivos de application
