use crate::error::AuthError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Persistent auth credential store.
///
/// Stores tokens per provider in a JSON file at `~/.config/theo/auth.json`.
#[derive(Debug, Clone)]
pub struct AuthStore {
    path: PathBuf,
}

/// A stored auth entry for a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AuthEntry {
    /// OAuth tokens (access + refresh).
    #[serde(rename = "oauth")]
    OAuth {
        access_token: String,
        refresh_token: Option<String>,
        /// Expiry as unix timestamp in seconds.
        expires_at: Option<u64>,
        account_id: Option<String>,
        scopes: Option<String>,
    },
    /// Manual API key.
    #[serde(rename = "api_key")]
    ApiKey { key: String },
}

impl AuthEntry {
    /// Check if OAuth tokens are expired.
    pub fn is_expired(&self) -> bool {
        match self {
            AuthEntry::OAuth {
                expires_at: Some(exp),
                ..
            } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                *exp <= now
            }
            _ => false,
        }
    }

    /// Get the bearer token (access_token for OAuth, key for API key).
    pub fn bearer_token(&self) -> &str {
        match self {
            AuthEntry::OAuth { access_token, .. } => access_token,
            AuthEntry::ApiKey { key } => key,
        }
    }

    /// Get the account ID if available.
    pub fn account_id(&self) -> Option<&str> {
        match self {
            AuthEntry::OAuth { account_id, .. } => account_id.as_deref(),
            AuthEntry::ApiKey { .. } => None,
        }
    }
}

/// The on-disk format.
#[derive(Debug, Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(flatten)]
    entries: HashMap<String, AuthEntry>,
}

impl AuthStore {
    /// Create a store at the default location (`~/.config/theo/auth.json`).
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("theo")
            .join("auth.json")
    }

    /// Create a store at a custom path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Create a store at the default location.
    pub fn open() -> Self {
        Self::new(Self::default_path())
    }

    /// Get the store file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Load all entries from disk.
    fn load(&self) -> Result<StoreFile, AuthError> {
        if !self.path.exists() {
            return Ok(StoreFile::default());
        }
        let content = std::fs::read_to_string(&self.path)
            .map_err(|e| AuthError::Storage(format!("read {}: {e}", self.path.display())))?;
        serde_json::from_str(&content)
            .map_err(|e| AuthError::Storage(format!("parse {}: {e}", self.path.display())))
    }

    /// Save all entries to disk.
    fn save(&self, store: &StoreFile) -> Result<(), AuthError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AuthError::Storage(format!("mkdir {}: {e}", parent.display())))?;
        }
        let content = serde_json::to_string_pretty(store)
            .map_err(|e| AuthError::Storage(format!("serialize: {e}")))?;
        std::fs::write(&self.path, content)
            .map_err(|e| AuthError::Storage(format!("write {}: {e}", self.path.display())))?;

        // Set file permissions to 0600 (user-only) on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(&self.path, perms);
        }

        Ok(())
    }

    /// Get the auth entry for a provider.
    pub fn get(&self, provider: &str) -> Result<Option<AuthEntry>, AuthError> {
        let store = self.load()?;
        Ok(store.entries.get(provider).cloned())
    }

    /// Set the auth entry for a provider.
    pub fn set(&self, provider: &str, entry: AuthEntry) -> Result<(), AuthError> {
        let mut store = self.load()?;
        store.entries.insert(provider.to_string(), entry);
        self.save(&store)
    }

    /// Remove the auth entry for a provider.
    pub fn remove(&self, provider: &str) -> Result<(), AuthError> {
        let mut store = self.load()?;
        store.entries.remove(provider);
        self.save(&store)
    }

    /// List all provider IDs that have stored credentials.
    pub fn providers(&self) -> Result<Vec<String>, AuthError> {
        let store = self.load()?;
        Ok(store.entries.keys().cloned().collect())
    }

    /// Update only the tokens for an existing OAuth entry.
    pub fn update_tokens(
        &self,
        provider: &str,
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<u64>,
    ) -> Result<(), AuthError> {
        let mut store = self.load()?;
        match store.entries.get_mut(provider) {
            Some(AuthEntry::OAuth {
                access_token: at,
                refresh_token: rt,
                expires_at: ea,
                ..
            }) => {
                *at = access_token;
                if let Some(new_rt) = refresh_token {
                    *rt = Some(new_rt);
                }
                *ea = expires_at;
            }
            _ => {
                store.entries.insert(
                    provider.to_string(),
                    AuthEntry::OAuth {
                        access_token,
                        refresh_token,
                        expires_at,
                        account_id: None,
                        scopes: None,
                    },
                );
            }
        }
        self.save(&store)
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
    fn test_empty_store() {
        let (store, _dir) = temp_store();
        assert!(store.get("openai").unwrap().is_none());
        assert!(store.providers().unwrap().is_empty());
    }

    #[test]
    fn test_set_and_get_oauth() {
        let (store, _dir) = temp_store();
        store
            .set(
                "openai",
                AuthEntry::OAuth {
                    access_token: "at_123".to_string(),
                    refresh_token: Some("rt_456".to_string()),
                    expires_at: Some(9999999999),
                    account_id: Some("acc_789".to_string()),
                    scopes: Some("openid profile".to_string()),
                },
            )
            .unwrap();

        let entry = store.get("openai").unwrap().unwrap();
        assert_eq!(entry.bearer_token(), "at_123");
        assert_eq!(entry.account_id(), Some("acc_789"));
        assert!(!entry.is_expired());
    }

    #[test]
    fn test_set_and_get_api_key() {
        let (store, _dir) = temp_store();
        store
            .set(
                "anthropic",
                AuthEntry::ApiKey {
                    key: "sk-ant-123".to_string(),
                },
            )
            .unwrap();

        let entry = store.get("anthropic").unwrap().unwrap();
        assert_eq!(entry.bearer_token(), "sk-ant-123");
        assert!(entry.account_id().is_none());
    }

    #[test]
    fn test_expired_token() {
        let entry = AuthEntry::OAuth {
            access_token: "expired".to_string(),
            refresh_token: None,
            expires_at: Some(1), // epoch + 1 second = definitely expired
            account_id: None,
            scopes: None,
        };
        assert!(entry.is_expired());
    }

    #[test]
    fn test_remove_provider() {
        let (store, _dir) = temp_store();
        store
            .set(
                "openai",
                AuthEntry::ApiKey {
                    key: "k".to_string(),
                },
            )
            .unwrap();
        assert!(store.get("openai").unwrap().is_some());
        store.remove("openai").unwrap();
        assert!(store.get("openai").unwrap().is_none());
    }

    #[test]
    fn test_update_tokens() {
        let (store, _dir) = temp_store();
        store
            .set(
                "openai",
                AuthEntry::OAuth {
                    access_token: "old".to_string(),
                    refresh_token: Some("old_rt".to_string()),
                    expires_at: Some(100),
                    account_id: Some("acc".to_string()),
                    scopes: None,
                },
            )
            .unwrap();

        store
            .update_tokens(
                "openai",
                "new_at".to_string(),
                Some("new_rt".to_string()),
                Some(200),
            )
            .unwrap();

        let entry = store.get("openai").unwrap().unwrap();
        assert_eq!(entry.bearer_token(), "new_at");
        // account_id preserved
        assert_eq!(entry.account_id(), Some("acc"));
    }

    #[test]
    fn test_multiple_providers() {
        let (store, _dir) = temp_store();
        store
            .set(
                "openai",
                AuthEntry::ApiKey {
                    key: "k1".to_string(),
                },
            )
            .unwrap();
        store
            .set(
                "anthropic",
                AuthEntry::ApiKey {
                    key: "k2".to_string(),
                },
            )
            .unwrap();

        let mut providers = store.providers().unwrap();
        providers.sort();
        assert_eq!(providers, vec!["anthropic", "openai"]);
    }
}
