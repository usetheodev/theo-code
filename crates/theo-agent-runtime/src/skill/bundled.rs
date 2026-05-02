//! Bundled skills — compiled into the binary.

use super::{SkillDefinition, SkillMode};

fn subagent(name: &str) -> SkillMode {
    SkillMode::SubAgent {
        agent_name: name.to_string(),
    }
}

pub fn bundled_skills() -> Vec<SkillDefinition> {
    vec![
        commit_skill(),
        test_skill(),
        review_skill(),
        build_skill(),
        explain_skill(),
        fix_skill(),
        refactor_skill(),
        init_skill(),
        doc_skill(),
        deps_skill(),
    ]
}

fn commit_skill() -> SkillDefinition {
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
    }
}

fn test_skill() -> SkillDefinition {
    SkillDefinition {
        name: "test".into(),
        trigger: "when the user asks to run tests, check tests, verify tests, or cargo test".into(),
        mode: subagent("verifier"),
        instructions: r#"## Test Skill

Run the project's test suite directly. Do NOT create tasks for this — just execute.

1. Look at the project root for Cargo.toml, package.json, pyproject.toml, etc. to detect the framework.
2. Run tests: `cargo test`, `npm test`, `pytest`, etc.
3. Report: passed/failed/skipped counts. If failures, show which tests failed and why (file:line, expected vs actual).

Be direct. Skip task management overhead for this workflow."#.into(),
    }
}

fn review_skill() -> SkillDefinition {
    SkillDefinition {
        name: "review".into(),
        trigger: "when the user asks for code review, review changes, or check code quality".into(),
        mode: subagent("reviewer"),
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
    }
}

fn build_skill() -> SkillDefinition {
    SkillDefinition {
        name: "build".into(),
        trigger: "when the user asks to build, compile, check build, cargo build, or cargo check".into(),
        mode: subagent("verifier"),
        instructions: r#"## Build Skill

1. Detect the build tool (cargo, npm, make, etc.).
2. Run the build command.
3. If build fails, analyze errors:
   - What type of error (syntax, type, linking)?
   - What file/line?
   - What's the likely fix?
4. If build succeeds, report: compilation time, warnings count.
5. If there are warnings, list the most important ones."#.into(),
    }
}

fn explain_skill() -> SkillDefinition {
    SkillDefinition {
        name: "explain".into(),
        trigger: "when the user asks to explain code, what does this do, how does this work, or describe the architecture".into(),
        mode: SkillMode::InContext,
        instructions: r#"## Explain Skill

1. Identify what the user wants explained (file, function, module, architecture).
2. Read the relevant code carefully.
3. Explain in this structure:
   - **Purpose**: what problem this solves
   - **How it works**: the flow / algorithm
   - **Inputs/Outputs**: what goes in, what comes out
   - **Notable details**: edge cases, design choices

## Rules
- Be concise. Don't repeat the code verbatim.
- Show example usage if non-obvious.
- If the code has bugs or smells, mention them."#.into(),
    }
}

fn fix_skill() -> SkillDefinition {
    SkillDefinition {
        name: "fix".into(),
        trigger: "when the user asks to fix a bug, debug an error, or troubleshoot".into(),
        mode: SkillMode::InContext,
        instructions: r#"## Fix Skill

1. Reproduce the bug if possible.
2. Read the error message carefully — what does it actually say?
3. Trace through the code to find the root cause.
4. Make a minimal fix — change only what's necessary.
5. Add a regression test BEFORE fixing (TDD: red → green → refactor).
6. Verify the fix doesn't break other tests."#.into(),
    }
}

fn refactor_skill() -> SkillDefinition {
    SkillDefinition {
        name: "refactor".into(),
        trigger: "when the user asks to refactor, clean up, improve code, or simplify".into(),
        mode: SkillMode::InContext,
        instructions: r#"## Refactor Skill

1. Identify the code to refactor and its current responsibilities.
2. Define the goal: extract function, simplify logic, remove duplication, improve naming.
3. Make small, atomic changes — one refactor at a time.
4. Run tests after EACH change to catch regressions early.
5. Keep behavior identical — refactoring changes structure, NOT behavior.

## Rules
- Don't refactor and add features in the same change.
- If tests don't exist, write them BEFORE refactoring.
- If a refactor requires multiple commits, do small reviewable steps."#.into(),
    }
}

fn init_skill() -> SkillDefinition {
    SkillDefinition {
        name: "init".into(),
        trigger: "when the user asks to initialize, set up, or scaffold a new project/feature".into(),
        mode: SkillMode::InContext,
        instructions: r#"## Init Skill

1. Ask what kind of project/feature (CLI, library, web service, etc.).
2. Generate a minimal scaffold:
   - Directory structure
   - Build manifest (Cargo.toml, package.json, etc.)
   - Entry point with hello-world
   - Test file with one passing test
3. Add minimal docs: README with how to build/run/test.
4. Verify the scaffold builds and tests pass before declaring done."#.into(),
    }
}

fn doc_skill() -> SkillDefinition {
    SkillDefinition {
        name: "doc".into(),
        trigger: "when the user asks to document, write docs, add comments, or write README".into(),
        mode: SkillMode::InContext,
        instructions: r#"## Doc Skill

1. Identify what needs documentation (function, module, project, API).
2. For functions: write doc comments that explain WHY (purpose, invariants, examples).
3. For modules: explain the role of the module in the larger system.
4. For projects: README with what/why/how (install, build, test, use).
5. Use code examples where helpful.

## Rules
- Write for the reader, not the writer.
- Don't document the obvious (getters, simple constructors).
- DO document: why (not just what), edge cases, non-obvious behavior."#.into(),
    }
}

fn deps_skill() -> SkillDefinition {
    SkillDefinition {
        name: "deps".into(),
        trigger: "when the user asks to check dependencies, audit packages, find vulnerabilities, or review Cargo.toml/package.json".into(),
        mode: subagent("explorer"),
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
    }
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
        assert_eq!(commit.mode, SkillMode::InContext);
    }

    #[test]
    fn test_is_subagent_verifier() {
        let skills = bundled_skills();
        let test = skills.iter().find(|s| s.name == "test").unwrap();
        match &test.mode {
            SkillMode::SubAgent { agent_name } => assert_eq!(agent_name, "verifier"),
            _ => panic!("expected SubAgent verifier"),
        }
    }

    #[test]
    fn review_is_subagent_reviewer() {
        let skills = bundled_skills();
        let review = skills.iter().find(|s| s.name == "review").unwrap();
        match &review.mode {
            SkillMode::SubAgent { agent_name } => assert_eq!(agent_name, "reviewer"),
            _ => panic!("expected SubAgent reviewer"),
        }
    }
}
