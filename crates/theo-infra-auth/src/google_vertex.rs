//! Google Vertex AI — Application Default Credentials (ADC).
//!
//! Resolves credentials via:
//! 1. GOOGLE_APPLICATION_CREDENTIALS env var (service account JSON)
//! 2. gcloud auth application-default login (user credentials)
//! 3. GCE metadata server (on GCP instances)
//!
//! Feature-gated: full ADC requires `gcp-auth` crate.
//! Without it, only env var token and gcloud CLI fallback work.

use crate::error::AuthError;
use crate::store::{AuthEntry, AuthStore};

const PROVIDER_ID: &str = "google-vertex";

#[derive(Debug, Clone)]
pub struct VertexConfig {
    pub project: String,
    pub location: String,
}

impl Default for VertexConfig {
    fn default() -> Self {
        Self {
            project: std::env::var("GOOGLE_CLOUD_PROJECT")
                .or_else(|_| std::env::var("GCP_PROJECT"))
                .or_else(|_| std::env::var("GCLOUD_PROJECT"))
                .unwrap_or_default(),
            location: std::env::var("GOOGLE_VERTEX_LOCATION")
                .or_else(|_| std::env::var("GOOGLE_CLOUD_LOCATION"))
                .or_else(|_| std::env::var("VERTEX_LOCATION"))
                .unwrap_or_else(|_| "us-central1".to_string()),
        }
    }
}

impl VertexConfig {
    /// Resolve the Vertex AI endpoint for this location.
    pub fn endpoint(&self) -> String {
        if self.location == "global" {
            "https://aiplatform.googleapis.com".to_string()
        } else {
            format!("https://{}-aiplatform.googleapis.com", self.location)
        }
    }
}

#[derive(Debug, Clone)]
pub struct VertexTokens {
    pub access_token: String,
    pub project: String,
    pub location: String,
}

pub struct GoogleVertexAuth {
    store: AuthStore,
    config: VertexConfig,
}

impl GoogleVertexAuth {
    pub fn new(store: AuthStore) -> Self {
        Self {
            store,
            config: VertexConfig::default(),
        }
    }

    pub fn with_config(store: AuthStore, config: VertexConfig) -> Self {
        Self { store, config }
    }

    pub fn with_default_store() -> Self {
        Self::new(AuthStore::open())
    }

    /// Try to get a token via gcloud CLI (fallback when gcp-auth crate not available).
    pub async fn get_token_via_gcloud(&self) -> Result<VertexTokens, AuthError> {
        let output = tokio::process::Command::new("gcloud")
            .args(["auth", "application-default", "print-access-token"])
            .output()
            .await
            .map_err(|e| AuthError::OAuth(format!("gcloud not available: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AuthError::OAuth(format!("gcloud auth failed: {stderr}")));
        }

        let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if token.is_empty() {
            return Err(AuthError::OAuth("gcloud returned empty token".to_string()));
        }

        // Store token (short-lived, ~1 hour)
        self.store.set(
            PROVIDER_ID,
            AuthEntry::OAuth {
                access_token: token.clone(),
                refresh_token: None,
                expires_at: Some(now_secs() + 3600),
                account_id: None,
                scopes: None,
            },
        )?;

        Ok(VertexTokens {
            access_token: token,
            project: self.config.project.clone(),
            location: self.config.location.clone(),
        })
    }

    pub fn get_tokens(&self) -> Result<Option<VertexTokens>, AuthError> {
        let entry = self.store.get(PROVIDER_ID)?;
        match entry {
            Some(AuthEntry::OAuth { access_token, expires_at, .. }) => {
                // Check if expired
                if let Some(exp) = expires_at {
                    if exp <= now_secs() {
                        return Ok(None); // Expired, need refresh
                    }
                }
                Ok(Some(VertexTokens {
                    access_token,
                    project: self.config.project.clone(),
                    location: self.config.location.clone(),
                }))
            }
            _ => Ok(None),
        }
    }

    pub fn has_valid_tokens(&self) -> bool {
        self.get_tokens().ok().flatten().is_some()
    }

    pub fn logout(&self) -> Result<(), AuthError> {
        self.store.remove(PROVIDER_ID)
    }

    pub fn config(&self) -> &VertexConfig { &self.config }
    pub fn provider_id() -> &'static str { PROVIDER_ID }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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
    fn vertex_config_default_location() {
        let config = VertexConfig {
            project: "test".to_string(),
            location: "us-central1".to_string(),
        };
        assert_eq!(config.endpoint(), "https://us-central1-aiplatform.googleapis.com");
    }

    #[test]
    fn vertex_config_global_location() {
        let config = VertexConfig {
            project: "test".to_string(),
            location: "global".to_string(),
        };
        assert_eq!(config.endpoint(), "https://aiplatform.googleapis.com");
    }

    #[test]
    fn vertex_store_and_retrieve() {
        let (store, _dir) = temp_store();
        store.set(PROVIDER_ID, AuthEntry::OAuth {
            access_token: "ya29.test".to_string(),
            refresh_token: None,
            expires_at: Some(9999999999),
            account_id: None,
            scopes: None,
        }).unwrap();

        let config = VertexConfig { project: "my-project".to_string(), location: "us-east1".to_string() };
        let auth = GoogleVertexAuth::with_config(store, config);
        let tokens = auth.get_tokens().unwrap().unwrap();
        assert_eq!(tokens.access_token, "ya29.test");
        assert_eq!(tokens.project, "my-project");
    }

    #[test]
    fn vertex_expired_token_returns_none() {
        let (store, _dir) = temp_store();
        store.set(PROVIDER_ID, AuthEntry::OAuth {
            access_token: "expired".to_string(),
            refresh_token: None,
            expires_at: Some(1),
            account_id: None,
            scopes: None,
        }).unwrap();

        let auth = GoogleVertexAuth::new(store);
        assert!(auth.get_tokens().unwrap().is_none());
    }

    #[test]
    fn vertex_logout() {
        let (store, _dir) = temp_store();
        store.set(PROVIDER_ID, AuthEntry::OAuth {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_at: Some(9999999999),
            account_id: None,
            scopes: None,
        }).unwrap();

        let auth = GoogleVertexAuth::new(store);
        assert!(auth.has_valid_tokens());
        auth.logout().unwrap();
        assert!(!auth.has_valid_tokens());
    }

    #[test]
    fn vertex_provider_id() {
        assert_eq!(GoogleVertexAuth::provider_id(), "google-vertex");
    }
}
