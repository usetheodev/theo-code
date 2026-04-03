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

1. Detect the project's test framework (cargo test, npm test, pytest, etc.).
2. Run the test suite.
3. If tests fail, analyze the failures:
   - Which tests failed?
   - What was expected vs actual?
   - What file/line is the issue?
4. Report results clearly: passed, failed, skipped counts.
5. If all pass, confirm with a summary."#.into(),
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
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_skills_count() {
        let skills = bundled_skills();
        assert_eq!(skills.len(), 5);
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
