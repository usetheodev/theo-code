//! Environment variable sanitization — strips sensitive vars before execution.
//!
//! Uses a whitelist approach: only explicitly allowed vars pass through.
//! ALWAYS_STRIPPED_ENV_PREFIXES are removed regardless of whitelist.

use theo_domain::sandbox::{ALWAYS_STRIPPED_ENV_PREFIXES, ProcessPolicy};

/// Compute the set of env vars to pass to the sandboxed process.
///
/// Returns (key, value) pairs for allowed variables.
/// Algorithm:
/// 1. Start with current environment
/// 2. Keep only vars in the allowed list
/// 3. Remove any var matching ALWAYS_STRIPPED_ENV_PREFIXES even if in allowed list
pub fn sanitized_env(policy: &ProcessPolicy) -> Vec<(String, String)> {
    let mut result = Vec::new();

    for (key, value) in std::env::vars() {
        // Check if variable is in the allowed list
        if !policy
            .allowed_env_vars
            .iter()
            .any(|allowed| allowed == &key)
        {
            continue;
        }

        // Even if allowed, strip if it matches ALWAYS_STRIPPED prefixes
        if is_always_stripped(&key) {
            continue;
        }

        result.push((key, value));
    }

    result
}

/// Check if a variable name matches any of the always-stripped prefixes.
fn is_always_stripped(var_name: &str) -> bool {
    ALWAYS_STRIPPED_ENV_PREFIXES
        .iter()
        .any(|prefix| var_name.starts_with(prefix) || var_name == *prefix)
}

/// Apply sanitized environment to a Command.
///
/// Clears all env vars and sets only the allowed ones.
pub fn apply_to_command(cmd: &mut std::process::Command, policy: &ProcessPolicy) {
    let allowed = sanitized_env(policy);
    cmd.env_clear();
    for (key, value) in allowed {
        cmd.env(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_policy() -> ProcessPolicy {
        ProcessPolicy::default()
    }

    #[test]
    fn sanitized_env_preserves_path() {
        let env = sanitized_env(&default_policy());
        let has_path = env.iter().any(|(k, _)| k == "PATH");
        // PATH should always be in the environment
        assert!(has_path, "PATH should be preserved");
    }

    #[test]
    fn sanitized_env_preserves_home() {
        let env = sanitized_env(&default_policy());
        let has_home = env.iter().any(|(k, _)| k == "HOME");
        assert!(has_home, "HOME should be preserved");
    }

    #[test]
    fn sanitized_env_strips_unlisted_vars() {
        // Set a custom var that's NOT in the whitelist
        unsafe { std::env::set_var("THEO_TEST_CUSTOM_VAR", "secret") };
        let env = sanitized_env(&default_policy());
        let has_custom = env.iter().any(|(k, _)| k == "THEO_TEST_CUSTOM_VAR");
        assert!(!has_custom, "Custom var should be stripped");
        unsafe { std::env::remove_var("THEO_TEST_CUSTOM_VAR") };
    }

    #[test]
    fn sanitized_env_strips_aws_even_if_in_whitelist() {
        // Even if someone adds AWS_ to allowed, ALWAYS_STRIPPED takes precedence
        let mut policy = default_policy();
        policy
            .allowed_env_vars
            .push("AWS_SECRET_ACCESS_KEY".to_string());

        unsafe { std::env::set_var("AWS_SECRET_ACCESS_KEY", "AKIAIOSFODNN7EXAMPLE") };
        let env = sanitized_env(&policy);
        let has_aws = env.iter().any(|(k, _)| k == "AWS_SECRET_ACCESS_KEY");
        assert!(!has_aws, "AWS vars should ALWAYS be stripped");
        unsafe { std::env::remove_var("AWS_SECRET_ACCESS_KEY") };
    }

    #[test]
    fn sanitized_env_strips_github_token() {
        unsafe { std::env::set_var("GITHUB_TOKEN", "ghp_test123") };
        let env = sanitized_env(&default_policy());
        let has_token = env.iter().any(|(k, _)| k == "GITHUB_TOKEN");
        assert!(!has_token, "GITHUB_TOKEN should be stripped");
        unsafe { std::env::remove_var("GITHUB_TOKEN") };
    }

    #[test]
    fn sanitized_env_strips_openai_api_key() {
        unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test123") };
        let env = sanitized_env(&default_policy());
        let has_key = env.iter().any(|(k, _)| k == "OPENAI_API_KEY");
        assert!(!has_key, "OPENAI_API_KEY should be stripped");
        unsafe { std::env::remove_var("OPENAI_API_KEY") };
    }

    #[test]
    fn sanitized_env_strips_anthropic_api_key() {
        unsafe { std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test123") };
        let env = sanitized_env(&default_policy());
        let has_key = env.iter().any(|(k, _)| k == "ANTHROPIC_API_KEY");
        assert!(!has_key, "ANTHROPIC_API_KEY should be stripped");
        unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };
    }

    #[test]
    fn sanitized_env_empty_whitelist_returns_nothing() {
        let policy = ProcessPolicy {
            max_processes: 0,
            max_memory_bytes: 0,
            max_cpu_seconds: 0,
            max_file_size_bytes: 0,
            allowed_env_vars: vec![],
        };
        let env = sanitized_env(&policy);
        assert!(env.is_empty(), "Empty whitelist should return no vars");
    }

    #[test]
    fn is_always_stripped_matches_prefixes() {
        assert!(is_always_stripped("AWS_SECRET_ACCESS_KEY"));
        assert!(is_always_stripped("AWS_ACCESS_KEY_ID"));
        assert!(is_always_stripped("GITHUB_TOKEN"));
        assert!(is_always_stripped("OPENAI_API_KEY"));
        assert!(is_always_stripped("ANTHROPIC_API_KEY"));
        assert!(is_always_stripped("DOCKER_HOST"));
    }

    #[test]
    fn is_always_stripped_does_not_match_safe_vars() {
        assert!(!is_always_stripped("PATH"));
        assert!(!is_always_stripped("HOME"));
        assert!(!is_always_stripped("USER"));
        assert!(!is_always_stripped("LANG"));
    }

    #[test]
    fn apply_to_command_clears_and_sets() {
        let policy = default_policy();
        let mut cmd = std::process::Command::new("env");
        apply_to_command(&mut cmd, &policy);
        // We can't easily verify the internal state of Command,
        // but we can verify it doesn't panic
    }
}
