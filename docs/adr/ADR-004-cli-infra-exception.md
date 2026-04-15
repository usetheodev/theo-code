# ADR-004: CLI Exception to Apps-Never-Import-Infra Rule

- **Status**: Accepted
- **Date**: 2026-04-11
- **Deciders**: Meeting 20260411-103954
- **Context**: CLI Professionalization Plan, architectural boundary review

## Context

The project's architectural rule (per `.claude/CLAUDE.md`) states:

> Apps NEVER import engine/infra crates directly — go through `theo-application`

However, `apps/theo-cli/Cargo.toml` currently imports:

```toml
theo-infra-auth = { path = "../../crates/theo-infra-auth" }
theo-infra-llm = { path = "../../crates/theo-infra-llm" }
```

This was flagged by arch-validator in the CLI professionalization meeting.

## Decision

**Accept the exception**. The CLI is allowed to import `theo-infra-auth` and `theo-infra-llm` directly, but only for:

1. **Bootstrap concerns** that must happen before `theo-application` can construct use cases:
   - OAuth PKCE flow (device-code login)
   - Provider registry inspection (listing models before connecting)
   - Credential loading from config

2. **Presentation concerns** that are CLI-specific:
   - Masking API keys in `/status` output
   - Rendering provider names/models for `/model` switcher

The CLI MUST NOT import infra for business logic — agent loop, tool execution, and governance all go through `theo-application`.

## Rationale

### Why allow the exception

Bootstrap is circular by nature:
- `theo-application::AgentRuntime::new()` needs a constructed `LlmClient`
- `LlmClient` needs auth credentials
- Auth credentials need user interaction (OAuth flow)
- User interaction needs CLI rendering

Forcing all of this through `theo-application` would create a "bootstrap use case" that is nothing more than a pass-through with an awkward API. That is worse than the documented exception.

### Why scope the exception narrowly

The risk of "allow one exception, get ten" is real. This ADR scopes the exception to:

- Crates: `theo-infra-auth`, `theo-infra-llm` only
- Concerns: bootstrap + presentation only
- Prohibited: business logic, agent loop, tool orchestration, governance

Any new infra import in `apps/theo-cli` (e.g., `theo-infra-graph`) requires amending this ADR.

## Alternatives Considered

### Alternative 1: Route everything through theo-application
- **Rejected**: Creates bootstrap-only use cases that leak infra types anyway. Adds indirection with no benefit.

### Alternative 2: Create a `theo-bootstrap` crate
- **Rejected**: Premature. Just another layer to pass through. YAGNI until we have 2+ apps needing the same bootstrap.

### Alternative 3: Move auth/provider config into `theo-application`
- **Rejected**: Would pull OAuth device flow UI concerns into the application layer, which should be presentation-agnostic.

## Consequences

### Positive
- Pragmatic — no bureaucratic indirection for bootstrap
- Clear scope — exception is documented and auditable
- Testable — arch-validator CI rule whitelists these two imports only

### Negative
- Arch-validator cannot be a simple "apps/ must not depend on infra" rule
- Contributors must read this ADR before adding infra imports

### Enforcement

Arch-validator CI rule (pseudocode):

```
for crate in apps/*:
  for dep in crate.dependencies:
    if dep starts with "theo-infra-":
      if (crate, dep) not in ADR_004_WHITELIST:
        FAIL "Infra import not in ADR-004 exception list"
```

Whitelist: `{("theo-cli", "theo-infra-auth"), ("theo-cli", "theo-infra-llm")}`

### Future Review

If `theo-desktop` also needs direct infra access, extend this ADR. If it grows beyond 2 apps × 2 crates, reconsider the `theo-bootstrap` crate alternative.

## References

- Plan: `docs/roadmap/cli-professionalization.md`
- Boundary rule: `.claude/CLAUDE.md` §Dependency Rules
- Meeting: `.claude/meetings/20260411-103954-cli-professionalization.md`
- Current offenders: `apps/theo-cli/Cargo.toml` L11-12
