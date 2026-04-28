//! Onboarding / bootstrap — part of `PLAN_AUTO_EVOLUTION_SOTA`.
//!
//! Pattern ported from OpenClaw's `BOOTSTRAP.md` workflow (docs.openclaw.ai).
//! On first session with a new user we:
//!
//! 1. Check whether `USER.md` exists and has content.
//! 2. If not, prepend a `BOOTSTRAP_PROMPT` to the system message so the
//!    agent asks a short Q&A before starting the requested task.
//! 3. Populated `USER.md` is the signal that onboarding is done; the
//!    prompt is never prepended again.
//!
//! Anti-footgun: we do NOT auto-persist `BOOTSTRAP.md` on disk. The
//! prompt is injected at runtime; once the user's answers end up in
//! `USER.md`, `needs_bootstrap` returns false on the next boot.
//!
//! Auto-improvement reminder is a separate sibling prompt for the
//! `UserPromptSubmit` hook — injected periodically to keep
//! memory-save behaviour fresh without bloating the system prompt.

use std::path::{Path, PathBuf};

/// Minimum byte count below which `USER.md` is treated as empty. 50
/// bytes catches files that contain only the frontmatter skeleton
/// (`---\n---\n`) without actual user facts.
pub const USER_MD_MIN_CONTENT_BYTES: usize = 50;

/// Canonical filename for the user profile.
pub const USER_MD_FILENAME: &str = "USER.md";

/// Prompt injected on the first session to collect user profile.
/// Kept short — the agent is instructed to ask one question at a time
/// rather than dump the whole Q&A at once.
pub const BOOTSTRAP_PROMPT: &str = concat!(
    "[theo onboarding] This is your first session with this user.\n",
    "Before helping, gather minimal context to personalise your behaviour.\n",
    "Ask ONE question at a time, wait for the answer, then ask the next.\n\n",
    "Topics to cover:\n",
    "1. Role and work context (what they build, tech stack).\n",
    "2. Behaviour preferences (terse vs. verbose, autonomous vs. ask-first).\n",
    "3. Hard boundaries (what NOT to do, destructive ops).\n",
    "4. Communication preferences (language, formality).\n\n",
    "After collecting answers, call the memory tool to persist them under\n",
    "USER.md. Once saved, confirm and proceed with the actual task.",
);

/// Auto-improvement reminder. Injected periodically on
/// `UserPromptSubmit` so the agent doesn't forget to save learnings.
pub const AUTO_IMPROVEMENT_REMINDER: &str = concat!(
    "[theo reminder] If this conversation reveals something about the user's\n",
    "preferences, workflow, or environment, save it to memory before ending\n",
    "the turn. When using a skill and finding it outdated, incomplete, or\n",
    "wrong, patch it immediately using skill_manage — don't wait to be asked.",
);

/// Path to the user profile file within a memory directory.
pub fn user_profile_path(memory_dir: &Path) -> PathBuf {
    memory_dir.join(USER_MD_FILENAME)
}

/// Returns `true` when the bootstrap Q&A should run. Cases that
/// trigger bootstrap:
/// - The memory directory does not exist yet.
/// - `USER.md` does not exist.
/// - `USER.md` exists but has less than [`USER_MD_MIN_CONTENT_BYTES`]
///   trimmed bytes (defensive — matches the "empty frontmatter only"
///   pattern OpenClaw documents).
///
/// Returns `false` otherwise so the prompt is injected only once per
/// user profile lifetime.
///
/// `THEO_SKIP_ONBOARDING=1` env var
/// short-circuits to `false` so headless / CI / E2E benchmarks bypass
/// the Q&A and execute the prompt literally. This is the "headless-direct"
/// mode the plan described.
pub fn needs_bootstrap(memory_dir: &Path) -> bool {
    // T3.3: env reads funnel through theo_domain::environment for
    // uniform truthy/falsey semantics.
    if theo_domain::environment::bool_var("THEO_SKIP_ONBOARDING", false) {
        return false;
    }
    let path = user_profile_path(memory_dir);
    match std::fs::read_to_string(&path) {
        Ok(raw) => raw.trim().len() < USER_MD_MIN_CONTENT_BYTES,
        Err(_) => true,
    }
}

/// Prepend the bootstrap prompt to a caller-owned system message. The
/// caller decides where to put the composed string (typically into
/// `AgentConfig.system_prompt` for the upcoming turn).
pub fn compose_bootstrap_system_prompt(existing: &str) -> String {
    if existing.trim().is_empty() {
        BOOTSTRAP_PROMPT.to_string()
    } else {
        format!("{BOOTSTRAP_PROMPT}\n\n---\n\n{existing}")
    }
}

// ---------------------------------------------------------------------------
// UserProfile — structured view over USER.md frontmatter.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verbosity {
    Terse,
    Normal,
    Verbose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Autonomy {
    AskFirst,
    Autonomous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Formality {
    Casual,
    Formal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferenceSet {
    pub verbosity: Verbosity,
    pub autonomy: Autonomy,
    pub formality: Formality,
}

impl Default for PreferenceSet {
    fn default() -> Self {
        Self {
            verbosity: Verbosity::Normal,
            autonomy: Autonomy::AskFirst,
            formality: Formality::Casual,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserProfile {
    pub role: Option<String>,
    pub tech_stack: Vec<String>,
    pub preferences: PreferenceSet,
    pub boundaries: Vec<String>,
    pub language: Option<String>,
    pub updated_at_unix: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("profile io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("profile parse error: {0}")]
    Parse(String),
}

impl UserProfile {
    /// Serialise this profile as an OpenClaw-style markdown file with
    /// YAML-ish frontmatter plus a `# User` section for human-readable
    /// notes. Designed to be both round-trippable and pleasant to
    /// review in a diff.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("---\n");
        if let Some(ref role) = self.role {
            out.push_str(&format!("role: {role}\n"));
        }
        if !self.tech_stack.is_empty() {
            out.push_str(&format!("tech_stack: {}\n", self.tech_stack.join(", ")));
        }
        out.push_str(&format!("verbosity: {}\n", verbosity_slug(&self.preferences.verbosity)));
        out.push_str(&format!("autonomy: {}\n", autonomy_slug(&self.preferences.autonomy)));
        out.push_str(&format!("formality: {}\n", formality_slug(&self.preferences.formality)));
        if let Some(ref lang) = self.language {
            out.push_str(&format!("language: {lang}\n"));
        }
        if !self.boundaries.is_empty() {
            out.push_str("boundaries:\n");
            for b in &self.boundaries {
                out.push_str(&format!("  - {b}\n"));
            }
        }
        out.push_str(&format!("updated_at_unix: {}\n", self.updated_at_unix));
        out.push_str("---\n\n");
        out.push_str("# User Profile\n\n");
        if let Some(ref role) = self.role {
            out.push_str(&format!("**Role:** {role}\n\n"));
        }
        if !self.tech_stack.is_empty() {
            out.push_str(&format!("**Tech stack:** {}\n\n", self.tech_stack.join(", ")));
        }
        if !self.boundaries.is_empty() {
            out.push_str("**Boundaries:**\n");
            for b in &self.boundaries {
                out.push_str(&format!("- {b}\n"));
            }
        }
        out
    }

    /// Parse a markdown file with YAML-ish frontmatter. Missing fields
    /// fall back to defaults; unknown fields are ignored. Returns an
    /// error only when the file cannot be read or the frontmatter
    /// delimiter is malformed.
    pub fn from_markdown(raw: &str) -> Result<Self, ProfileError> {
        let trimmed = raw.trim_start();
        if !trimmed.starts_with("---") {
            return Err(ProfileError::Parse("missing frontmatter open".into()));
        }
        let without_open = &trimmed[3..];
        let Some(end) = without_open.find("\n---") else {
            return Err(ProfileError::Parse("missing frontmatter close".into()));
        };
        let block = &without_open[..end];
        let mut profile = UserProfile::default();
        let mut collecting_boundaries = false;
        for line in block.lines() {
            let trimmed_line = line.trim_end();
            if collecting_boundaries {
                if let Some(rest) = trimmed_line.strip_prefix("  - ") {
                    profile.boundaries.push(rest.trim().to_string());
                    continue;
                } else if trimmed_line.is_empty() {
                    continue;
                } else {
                    collecting_boundaries = false;
                }
            }
            if trimmed_line.is_empty() || trimmed_line.starts_with('#') {
                continue;
            }
            if trimmed_line == "boundaries:" {
                collecting_boundaries = true;
                continue;
            }
            if let Some((k, v)) = trimmed_line.split_once(':') {
                let key = k.trim();
                let value = v.trim();
                match key {
                    "role" => profile.role = Some(value.to_string()),
                    "tech_stack" => {
                        profile.tech_stack = value
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    "verbosity" => {
                        profile.preferences.verbosity = verbosity_from_slug(value);
                    }
                    "autonomy" => {
                        profile.preferences.autonomy = autonomy_from_slug(value);
                    }
                    "formality" => {
                        profile.preferences.formality = formality_from_slug(value);
                    }
                    "language" => profile.language = Some(value.to_string()),
                    "updated_at_unix" => {
                        profile.updated_at_unix = value.parse().unwrap_or(0);
                    }
                    _ => {} // Unknown keys silently ignored.
                }
            }
        }
        Ok(profile)
    }
}

fn verbosity_slug(v: &Verbosity) -> &'static str {
    match v {
        Verbosity::Terse => "terse",
        Verbosity::Normal => "normal",
        Verbosity::Verbose => "verbose",
    }
}

fn verbosity_from_slug(s: &str) -> Verbosity {
    match s.trim().to_lowercase().as_str() {
        "terse" => Verbosity::Terse,
        "verbose" => Verbosity::Verbose,
        _ => Verbosity::Normal,
    }
}

fn autonomy_slug(a: &Autonomy) -> &'static str {
    match a {
        Autonomy::AskFirst => "ask_first",
        Autonomy::Autonomous => "autonomous",
    }
}

fn autonomy_from_slug(s: &str) -> Autonomy {
    match s.trim().to_lowercase().as_str() {
        "autonomous" => Autonomy::Autonomous,
        _ => Autonomy::AskFirst,
    }
}

fn formality_slug(f: &Formality) -> &'static str {
    match f {
        Formality::Casual => "casual",
        Formality::Formal => "formal",
    }
}

fn formality_from_slug(s: &str) -> Formality {
    match s.trim().to_lowercase().as_str() {
        "formal" => Formality::Formal,
        _ => Formality::Casual,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Serializes the three tests that mutate `THEO_SKIP_ONBOARDING`
    /// (a production env var read by `needs_bootstrap`). Without this
    /// lock, cargo's parallel test runner makes them race with each
    /// other AND with any other test in the module that calls
    /// `needs_bootstrap` — same flake class as the wiki/compiler one
    /// fixed in commit 8025a70.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ── AC-5.1 ─────────────────────────────────────────────────
    #[test]
    fn test_needs_bootstrap_true_when_dir_missing() {
        let _guard = env_lock();
        let tmp = tempfile::tempdir().expect("tmp");
        let missing = tmp.path().join("not-created");
        assert!(needs_bootstrap(&missing));
    }

    #[test]
    fn test_needs_bootstrap_true_when_user_md_absent() {
        let _guard = env_lock();
        let tmp = tempfile::tempdir().expect("tmp");
        assert!(needs_bootstrap(tmp.path()));
    }

    #[test]
    fn test_needs_bootstrap_true_when_user_md_only_has_frontmatter() {
        let _guard = env_lock();
        let tmp = tempfile::tempdir().expect("tmp");
        std::fs::write(tmp.path().join(USER_MD_FILENAME), "---\n---\n").unwrap();
        assert!(needs_bootstrap(tmp.path()));
    }

    // ── AC-5.5 ─────────────────────────────────────────────────
    #[test]
    fn test_needs_bootstrap_false_when_user_md_populated() {
        let _guard = env_lock();
        let tmp = tempfile::tempdir().expect("tmp");
        let body = "---\nrole: data-scientist\n---\n# User\nA very useful description here.";
        std::fs::write(tmp.path().join(USER_MD_FILENAME), body).unwrap();
        assert!(!needs_bootstrap(tmp.path()));
    }

    // ── THEO_SKIP_ONBOARDING bypass ──
    #[test]
    fn needs_bootstrap_returns_false_when_skip_env_set() {
        // SAFETY: holding `ENV_LOCK` serialises every test in this
        // module that reads `THEO_SKIP_ONBOARDING` via
        // `needs_bootstrap`, so for the duration of this test no
        // other thread reads the variable.
        let _guard = env_lock();
        let tmp = tempfile::tempdir().expect("tmp");
        // No USER.md → would normally return true.
        unsafe { std::env::set_var("THEO_SKIP_ONBOARDING", "1"); }
        let result = needs_bootstrap(tmp.path());
        // SAFETY: serialized by `let _guard = env_lock();` at the top of this test; the env mutex makes mutations single-threaded for the lifetime of the guard.
        unsafe { std::env::remove_var("THEO_SKIP_ONBOARDING"); }
        assert!(!result, "THEO_SKIP_ONBOARDING=1 must skip bootstrap");
    }

    #[test]
    fn needs_bootstrap_returns_true_when_skip_env_set_to_zero() {
        // SAFETY: see `needs_bootstrap_returns_false_when_skip_env_set`.
        let _guard = env_lock();
        let tmp = tempfile::tempdir().expect("tmp");
        unsafe { std::env::set_var("THEO_SKIP_ONBOARDING", "0"); }
        let result = needs_bootstrap(tmp.path());
        unsafe { std::env::remove_var("THEO_SKIP_ONBOARDING"); }
        assert!(result, "THEO_SKIP_ONBOARDING=0 keeps default behavior");
    }

    #[test]
    fn needs_bootstrap_returns_false_when_skip_env_set_to_true_lowercase() {
        // SAFETY: see `needs_bootstrap_returns_false_when_skip_env_set`.
        let _guard = env_lock();
        let tmp = tempfile::tempdir().expect("tmp");
        unsafe { std::env::set_var("THEO_SKIP_ONBOARDING", "true"); }
        let result = needs_bootstrap(tmp.path());
        unsafe { std::env::remove_var("THEO_SKIP_ONBOARDING"); }
        // Any non-zero / non-false value enables skip.
        assert!(!result);
    }

    // ── AC-5.2 ─────────────────────────────────────────────────
    #[test]
    fn test_compose_system_prompt_prepends_bootstrap() {
        let out = compose_bootstrap_system_prompt("You are helpful.");
        assert!(out.starts_with(BOOTSTRAP_PROMPT));
        assert!(out.contains("You are helpful."));
    }

    #[test]
    fn test_compose_system_prompt_empty_existing_returns_bootstrap_only() {
        let out = compose_bootstrap_system_prompt("");
        assert_eq!(out, BOOTSTRAP_PROMPT);
    }

    // ── AC-5.3 ─────────────────────────────────────────────────
    #[test]
    fn test_bootstrap_prompt_mentions_four_topics() {
        // Defense against accidental prompt edits that drop the Q&A.
        assert!(BOOTSTRAP_PROMPT.contains("1."));
        assert!(BOOTSTRAP_PROMPT.contains("2."));
        assert!(BOOTSTRAP_PROMPT.contains("3."));
        assert!(BOOTSTRAP_PROMPT.contains("4."));
        assert!(BOOTSTRAP_PROMPT.contains("ONE question at a time"));
    }

    // ── AC-5.4 ─────────────────────────────────────────────────
    #[test]
    fn test_user_profile_round_trips_markdown() {
        let profile = UserProfile {
            role: Some("staff engineer".into()),
            tech_stack: vec!["rust".into(), "typescript".into()],
            preferences: PreferenceSet {
                verbosity: Verbosity::Terse,
                autonomy: Autonomy::Autonomous,
                formality: Formality::Casual,
            },
            boundaries: vec![
                "no force-push to main".into(),
                "no dependency churn".into(),
            ],
            language: Some("pt-BR".into()),
            updated_at_unix: 1_745_000_000,
        };
        let md = profile.to_markdown();
        let parsed = UserProfile::from_markdown(&md).expect("round trip");
        assert_eq!(parsed, profile);
    }

    #[test]
    fn test_user_profile_partial_fields_default_the_rest() {
        let md = "---\nrole: backend dev\n---\n# User\nNotes here";
        let parsed = UserProfile::from_markdown(md).expect("parse");
        assert_eq!(parsed.role.as_deref(), Some("backend dev"));
        assert_eq!(parsed.preferences, PreferenceSet::default());
        assert_eq!(parsed.tech_stack, Vec::<String>::new());
    }

    #[test]
    fn test_user_profile_unknown_keys_are_ignored() {
        let md = "---\nrole: dev\nexotic_key: foo\nverbosity: verbose\n---\n# User";
        let parsed = UserProfile::from_markdown(md).expect("parse");
        assert_eq!(parsed.role.as_deref(), Some("dev"));
        assert_eq!(parsed.preferences.verbosity, Verbosity::Verbose);
    }

    #[test]
    fn test_user_profile_rejects_missing_frontmatter() {
        let md = "# User\nNo frontmatter";
        assert!(UserProfile::from_markdown(md).is_err());
    }

    // ── AC-5.6 ─────────────────────────────────────────────────
    #[test]
    fn test_auto_improvement_reminder_is_non_empty_and_mentions_memory() {
        assert!(AUTO_IMPROVEMENT_REMINDER.contains("save it to memory"));
        assert!(AUTO_IMPROVEMENT_REMINDER.contains("skill_manage"));
    }
}
