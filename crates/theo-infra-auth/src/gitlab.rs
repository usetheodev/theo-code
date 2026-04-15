//! GitLab AI — OAuth or API token authentication.
//!
//! Supports custom GitLab instances via GITLAB_INSTANCE_URL.

use crate::error::AuthError;
use crate::store::{AuthEntry, AuthStore};

const DEFAULT_INSTANCE: &str = "https://gitlab.com";
const PROVIDER_ID: &str = "gitlab";

#[derive(Debug, Clone)]
pub struct GitLabConfig {
    pub instance_url: String,
}

impl Default for GitLabConfig {
    fn default() -> Self {
        Self {
            instance_url: std::env::var("GITLAB_INSTANCE_URL")
                .unwrap_or_else(|_| DEFAULT_INSTANCE.to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GitLabTokens {
    pub token: String,
    pub instance_url: String,
}

pub struct GitLabAuth {
    store: AuthStore,
    config: GitLabConfig,
}

impl GitLabAuth {
    pub fn new(store: AuthStore) -> Self {
        Self {
            store,
            config: GitLabConfig::default(),
        }
    }

    pub fn with_config(store: AuthStore, config: GitLabConfig) -> Self {
        Self { store, config }
    }

    pub fn with_default_store() -> Self {
        Self::new(AuthStore::open())
    }

    pub fn get_tokens(&self) -> Result<Option<GitLabTokens>, AuthError> {
        // Check store first, then env var
        let entry = self.store.get(PROVIDER_ID)?;
        match entry {
            Some(AuthEntry::OAuth { access_token, .. }) => Ok(Some(GitLabTokens {
                token: access_token,
                instance_url: self.config.instance_url.clone(),
            })),
            Some(AuthEntry::ApiKey { key }) => Ok(Some(GitLabTokens {
                token: key,
                instance_url: self.config.instance_url.clone(),
            })),
            None => {
                // Fallback to env var
                if let Ok(token) = std::env::var("GITLAB_TOKEN") {
                    Ok(Some(GitLabTokens {
                        token,
                        instance_url: self.config.instance_url.clone(),
                    }))
                } else {
                    Ok(None)
                }
            }
        }
    }

    pub fn has_tokens(&self) -> bool {
        self.get_tokens().ok().flatten().is_some()
    }

    pub fn set_token(&self, token: String) -> Result<(), AuthError> {
        self.store
            .set(PROVIDER_ID, AuthEntry::ApiKey { key: token })
    }

    pub fn logout(&self) -> Result<(), AuthError> {
        self.store.remove(PROVIDER_ID)
    }

    pub fn provider_id() -> &'static str {
        PROVIDER_ID
    }
    pub fn instance_url(&self) -> &str {
        &self.config.instance_url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (AuthStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        (AuthStore::new(path), dir)
    }

    #[test]
    fn gitlab_default_instance() {
        let config = GitLabConfig {
            instance_url: DEFAULT_INSTANCE.to_string(),
        };
        assert_eq!(config.instance_url, "https://gitlab.com");
    }

    #[test]
    fn gitlab_store_and_retrieve() {
        let (store, _dir) = temp_store();
        let auth = GitLabAuth::new(store);
        auth.set_token("glpat-test".to_string()).unwrap();
        let tokens = auth.get_tokens().unwrap().unwrap();
        assert_eq!(tokens.token, "glpat-test");
    }

    #[test]
    fn gitlab_logout() {
        let (store, _dir) = temp_store();
        let auth = GitLabAuth::new(store);
        auth.set_token("test".to_string()).unwrap();
        assert!(auth.has_tokens());
        auth.logout().unwrap();
        assert!(!auth.has_tokens());
    }

    #[test]
    fn gitlab_provider_id() {
        assert_eq!(GitLabAuth::provider_id(), "gitlab");
    }
}
