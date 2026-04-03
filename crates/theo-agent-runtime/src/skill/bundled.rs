//! Bundled skills — compiled into the binary.

use super::{SkillDefinition, SkillMode};
use crate::subagent::SubAgentRole;

pub fn bundled_skills() -> Vec<SkillDefinition> {
    vec![
        SkillDefinition {
            name: "commit".into(),
            trigger: "when the user asks to commit, save changes, create a commit, or push code".into(),
            mode: SkillMode::InContext,
            instructions: r#"## Commit Skill

1. Run `git status` and `git diff --stat` to see what changed.
2. Analyze the changes and classify: feature, fix, refactor, docs, test, chore.
3. Write a commit message following Conventional Commits: `type(scope): description`.
4. Stage relevant files — prefer specific files over `git add -A`.
5. Create the commit with the message.
6. Show the commit hash and summary.

## Rules
- NEVER commit files that may contain secrets (.env, credentials, keys).
- NEVER force push to main/master.
- NEVER use --no-verify or skip hooks.
- If there are no changes to commit, tell the user."#.into(),
        },
        SkillDefinition {
            name: "test".into(),
            trigger: "when the user asks to run tests, check tests, verify tests, or cargo test".into(),
            mode: SkillMode::SubAgent { role: SubAgentRole::Verifier },
            instructions: r#"## Test Skill

Run the project's test suite directly. Do NOT create tasks for this — just execute.

1. Look at the project root for Cargo.toml, package.json, pyproject.toml, etc. to detect the framework.
2. Run tests: `cargo test`, `npm test`, `pytest`, etc.
3. Report: passed/failed/skipped counts. If failures, show which tests failed and why (file:line, expected vs actual).

Be direct. Skip task management overhead for this workflow."#.into(),
        },
        SkillDefinition {
            name: "review".into(),
            trigger: "when the user asks for code review, review changes, or check code quality".into(),
            mode: SkillMode::SubAgent { role: SubAgentRole::Reviewer },
            instructions: r#"## Code Review Skill

1. Identify what files changed (git diff or recent edits).
2. Read each changed file carefully.
3. Look for:
   - Bugs or logic errors
   - Security vulnerabilities
   - Performance issues
   - Code style / readability
   - Missing error handling
   - Missing tests
4. Classify findings by severity: critical, major, minor, suggestion.
5. Report findings with file:line references."#.into(),
        },
        SkillDefinition {
            name: "build".into(),
            trigger: "when the user asks to build, compile, check build, cargo build, or cargo check".into(),
            mode: SkillMode::SubAgent { role: SubAgentRole::Verifier },
            instructions: r#"## Build Skill

1. Detect the build tool (cargo, npm, make, etc.).
2. Run the build command.
3. If build fails, analyze errors:
   - What type of error (syntax, type, linking)?
   - What file/line?
   - What's the likely fix?
4. If build succeeds, report: compilation time, warnings count.
5. If there are warnings, list the most important ones."#.into(),
        },
        SkillDefinition {
            name: "explain".into(),
            trigger: "when the user asks to explain code, what does this do, how does this work, or describe the architecture".into(),
            mode: SkillMode::InContext,
            instructions: r#"## Explain Skill

1. Identify what the user wants explained (file, function, module, architecture).
2. Read the relevant code thoroughly.
3. Explain in clear language:
   - What it does (purpose)
   - How it works (mechanism)
   - Why it's designed this way (rationale)
   - How it connects to other parts (dependencies)
4. Use examples if helpful.
5. Keep the explanation concise but complete."#.into(),
        },
        SkillDefinition {
            name: "fix".into(),
            trigger: "when the user asks to fix a bug, debug an error, resolve an issue, or repair broken code".into(),
            mode: SkillMode::InContext,
            instructions: r#"## Fix Skill

1. Understand the bug: read the error message, stack trace, or user description carefully.
2. Locate the source: use `grep`, `read`, and `glob` to find the relevant code.
3. Diagnose: use `think` to reason about the root cause before making changes.
4. Fix: make the minimal change that resolves the issue.
5. Verify: run tests or the failing command to confirm the fix works.
6. Report: explain what was wrong and what you changed.

## Rules
- Fix the root cause, not the symptom.
- Make the smallest possible change.
- If unsure about the cause, ask the user before editing."#.into(),
        },
        SkillDefinition {
            name: "refactor".into(),
            trigger: "when the user asks to refactor, clean up, reorganize, simplify, or improve code structure".into(),
            mode: SkillMode::InContext,
            instructions: r#"## Refactor Skill

1. Read the code to refactor thoroughly. Understand what it does before changing it.
2. Plan: use `think` to outline the refactoring steps.
3. Refactor incrementally — one change at a time, verify after each.
4. Preserve behavior: the refactored code must do exactly the same thing.
5. Run tests after refactoring to confirm nothing broke.

## Rules
- NEVER change behavior during a refactor. If the user wants new features, that's a separate task.
- Prefer small, focused changes over large rewrites.
- If tests don't exist for the code being refactored, mention this risk."#.into(),
        },
        SkillDefinition {
            name: "pr".into(),
            trigger: "when the user asks to create a pull request, open a PR, push changes, or submit for review".into(),
            mode: SkillMode::InContext,
            instructions: r#"## Pull Request Skill

1. Run `git status` and `git diff --stat` to understand what will be in the PR.
2. If on main/master, create a feature branch: `git checkout -b feat/description`.
3. Stage and commit changes (follow Conventional Commits).
4. Push: `git push -u origin <branch>`.
5. Create PR: `gh pr create --title "..." --body "..."` with:
   - Clear title (under 70 chars)
   - Summary of changes
   - Test plan

## Rules
- NEVER push directly to main/master.
- NEVER force push.
- If `gh` is not installed, tell the user to install it or create the PR manually.
- Include a test plan in the PR body."#.into(),
        },
        SkillDefinition {
            name: "doc".into(),
            trigger: "when the user asks to document, write docs, generate documentation, add comments, or update README".into(),
            mode: SkillMode::InContext,
            instructions: r#"## Documentation Skill

1. Identify what needs documentation (module, function, API, architecture).
2. Read the code thoroughly to understand it.
3. Write clear documentation:
   - For code: add doc comments (/// in Rust, /** */ in JS/TS, docstrings in Python).
   - For README: explain purpose, setup, usage, architecture.
   - For API: document endpoints, request/response formats, error codes.
4. Keep docs close to the code they describe.
5. Use examples where helpful.

## Rules
- Write for the reader, not the writer.
- Don't document the obvious (getters, simple constructors).
- DO document: why (not just what), edge cases, non-obvious behavior."#.into(),
        },
        SkillDefinition {
            name: "deps".into(),
            trigger: "when the user asks to check dependencies, audit packages, find vulnerabilities, or review Cargo.toml/package.json".into(),
            mode: SkillMode::SubAgent { role: SubAgentRole::Explorer },
            instructions: r#"## Dependencies Skill

Analyze project dependencies. Do NOT create tasks — just execute directly.

1. Read dependency files: Cargo.toml, Cargo.lock, package.json, package-lock.json, etc.
2. Run audit tools if available:
   - Rust: `cargo audit` (if installed), `cargo outdated`
   - Node: `npm audit`, `npm outdated`
3. Report:
   - Total dependency count
   - Known vulnerabilities (critical/high/medium/low)
   - Outdated packages with available updates
   - Unused dependencies (if detectable)
4. Recommend actions for critical findings."#.into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_skills_count() {
        let skills = bundled_skills();
        assert_eq!(skills.len(), 10);
    }

    #[test]
    fn bundled_skills_have_unique_names() {
        let skills = bundled_skills();
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len());
    }

    #[test]
    fn commit_is_in_context() {
        let skills = bundled_skills();
        let commit = skills.iter().find(|s| s.name == "commit").unwrap();
        assert!(matches!(commit.mode, SkillMode::InContext));
    }

    #[test]
    fn test_is_subagent_verifier() {
        let skills = bundled_skills();
        let test = skills.iter().find(|s| s.name == "test").unwrap();
        assert!(matches!(test.mode, SkillMode::SubAgent { role: SubAgentRole::Verifier }));
    }

    #[test]
    fn review_is_subagent_reviewer() {
        let skills = bundled_skills();
        let review = skills.iter().find(|s| s.name == "review").unwrap();
        assert!(matches!(review.mode, SkillMode::SubAgent { role: SubAgentRole::Reviewer }));
    }
}
