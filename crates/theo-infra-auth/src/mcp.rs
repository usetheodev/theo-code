//! MCP (Model Context Protocol) OAuth — PKCE flow with dynamic client registration.
//!
//! Stores tokens per-MCP-server in a separate mcp-auth.json file.
//! Supports: authorization_code + PKCE, refresh_token, dynamic client registration.

use crate::error::AuthError;
use crate::pkce::PkceChallenge;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

const MCP_AUTH_FILE: &str = "mcp-auth.json";
const MCP_CALLBACK_PORT: u16 = 19876;

/// Stored MCP auth data for a single server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerAuth {
    pub tokens: Option<McpTokens>,
    pub client_info: Option<McpClientInfo>,
    pub code_verifier: Option<String>,
    pub server_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpClientInfo {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub client_secret_expires_at: Option<u64>,
}

/// MCP auth store — separate from main auth.json.
pub struct McpAuthStore {
    path: PathBuf,
}

impl McpAuthStore {
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("theo")
            .join(MCP_AUTH_FILE)
    }

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn open() -> Self {
        Self::new(Self::default_path())
    }

    fn load(&self) -> Result<HashMap<String, McpServerAuth>, AuthError> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }
        let content = std::fs::read_to_string(&self.path)
            .map_err(|e| AuthError::Storage(format!("read mcp-auth: {e}")))?;
        serde_json::from_str(&content)
            .map_err(|e| AuthError::Storage(format!("parse mcp-auth: {e}")))
    }

    fn save(&self, data: &HashMap<String, McpServerAuth>) -> Result<(), AuthError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AuthError::Storage(format!("mkdir: {e}")))?;
        }
        let content = serde_json::to_string_pretty(data)
            .map_err(|e| AuthError::Storage(format!("serialize: {e}")))?;
        std::fs::write(&self.path, content)
            .map_err(|e| AuthError::Storage(format!("write: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }

    pub fn get(&self, server_name: &str) -> Result<Option<McpServerAuth>, AuthError> {
        let data = self.load()?;
        Ok(data.get(server_name).cloned())
    }

    pub fn set(&self, server_name: &str, auth: McpServerAuth) -> Result<(), AuthError> {
        let mut data = self.load()?;
        data.insert(server_name.to_string(), auth);
        self.save(&data)
    }

    pub fn remove(&self, server_name: &str) -> Result<(), AuthError> {
        let mut data = self.load()?;
        data.remove(server_name);
        self.save(&data)
    }

    pub fn servers(&self) -> Result<Vec<String>, AuthError> {
        let data = self.load()?;
        Ok(data.keys().cloned().collect())
    }
}

/// MCP OAuth client.
pub struct McpAuth {
    store: McpAuthStore,
}

impl McpAuth {
    pub fn new(store: McpAuthStore) -> Self {
        Self { store }
    }

    pub fn with_default_store() -> Self {
        Self::new(McpAuthStore::open())
    }

    /// Get stored tokens for an MCP server.
    pub fn get_tokens(&self, server_name: &str) -> Result<Option<McpTokens>, AuthError> {
        let auth = self.store.get(server_name)?;
        Ok(auth.and_then(|a| a.tokens))
    }

    /// Check if we have valid (non-expired) tokens for a server.
    pub fn has_valid_tokens(&self, server_name: &str) -> bool {
        self.get_tokens(server_name)
            .ok()
            .flatten()
            .is_some_and(|t| t.expires_at.map_or(true, |exp| exp > now_secs()))
    }

    /// Store tokens for an MCP server.
    pub fn save_tokens(&self, server_name: &str, tokens: McpTokens) -> Result<(), AuthError> {
        let mut auth = self.store.get(server_name)?.unwrap_or(McpServerAuth {
            tokens: None,
            client_info: None,
            code_verifier: None,
            server_url: None,
        });
        auth.tokens = Some(tokens);
        self.store.set(server_name, auth)
    }

    /// Store client registration info for an MCP server.
    pub fn save_client_info(
        &self,
        server_name: &str,
        info: McpClientInfo,
    ) -> Result<(), AuthError> {
        let mut auth = self.store.get(server_name)?.unwrap_or(McpServerAuth {
            tokens: None,
            client_info: None,
            code_verifier: None,
            server_url: None,
        });
        auth.client_info = Some(info);
        self.store.set(server_name, auth)
    }

    /// Generate PKCE challenge for OAuth flow.
    pub fn generate_pkce() -> PkceChallenge {
        PkceChallenge::generate()
    }

    /// Get the callback redirect URI.
    pub fn redirect_uri() -> String {
        format!("http://127.0.0.1:{MCP_CALLBACK_PORT}/mcp/oauth/callback")
    }

    /// Remove all auth data for an MCP server.
    pub fn logout(&self, server_name: &str) -> Result<(), AuthError> {
        self.store.remove(server_name)
    }

    /// List all MCP servers with stored auth.
    pub fn servers(&self) -> Result<Vec<String>, AuthError> {
        self.store.servers()
    }
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

    fn temp_store() -> (McpAuthStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp-auth.json");
        (McpAuthStore::new(path), dir)
    }

    #[test]
    fn mcp_store_empty() {
        let (store, _dir) = temp_store();
        assert!(store.get("test-server").unwrap().is_none());
        assert!(store.servers().unwrap().is_empty());
    }

    #[test]
    fn mcp_store_set_and_get() {
        let (store, _dir) = temp_store();
        store
            .set(
                "my-mcp",
                McpServerAuth {
                    tokens: Some(McpTokens {
                        access_token: "mcp-token".to_string(),
                        refresh_token: Some("mcp-refresh".to_string()),
                        expires_at: Some(9999999999),
                        scope: Some("read write".to_string()),
                    }),
                    client_info: Some(McpClientInfo {
                        client_id: "client-123".to_string(),
                        client_secret: Some("secret".to_string()),
                        client_secret_expires_at: None,
                    }),
                    code_verifier: None,
                    server_url: Some("https://mcp.example.com".to_string()),
                },
            )
            .unwrap();

        let auth = store.get("my-mcp").unwrap().unwrap();
        assert_eq!(auth.tokens.unwrap().access_token, "mcp-token");
        assert_eq!(auth.client_info.unwrap().client_id, "client-123");
    }

    #[test]
    fn mcp_auth_save_and_retrieve_tokens() {
        let (store, _dir) = temp_store();
        let auth = McpAuth::new(store);
        auth.save_tokens(
            "server1",
            McpTokens {
                access_token: "tk1".to_string(),
                refresh_token: None,
                expires_at: Some(9999999999),
                scope: None,
            },
        )
        .unwrap();

        assert!(auth.has_valid_tokens("server1"));
        let tokens = auth.get_tokens("server1").unwrap().unwrap();
        assert_eq!(tokens.access_token, "tk1");
    }

    #[test]
    fn mcp_auth_expired_tokens() {
        let (store, _dir) = temp_store();
        let auth = McpAuth::new(store);
        auth.save_tokens(
            "server1",
            McpTokens {
                access_token: "expired".to_string(),
                refresh_token: None,
                expires_at: Some(1),
                scope: None,
            },
        )
        .unwrap();

        assert!(!auth.has_valid_tokens("server1"));
    }

    #[test]
    fn mcp_auth_logout() {
        let (store, _dir) = temp_store();
        let auth = McpAuth::new(store);
        auth.save_tokens(
            "server1",
            McpTokens {
                access_token: "tk".to_string(),
                refresh_token: None,
                expires_at: None,
                scope: None,
            },
        )
        .unwrap();

        assert!(auth.has_valid_tokens("server1"));
        auth.logout("server1").unwrap();
        assert!(!auth.has_valid_tokens("server1"));
    }

    #[test]
    fn mcp_auth_multiple_servers() {
        let (store, _dir) = temp_store();
        let auth = McpAuth::new(store);
        auth.save_tokens(
            "s1",
            McpTokens {
                access_token: "a".to_string(),
                refresh_token: None,
                expires_at: None,
                scope: None,
            },
        )
        .unwrap();
        auth.save_tokens(
            "s2",
            McpTokens {
                access_token: "b".to_string(),
                refresh_token: None,
                expires_at: None,
                scope: None,
            },
        )
        .unwrap();

        let mut servers = auth.servers().unwrap();
        servers.sort();
        assert_eq!(servers, vec!["s1", "s2"]);
    }

    #[test]
    fn mcp_redirect_uri() {
        assert_eq!(
            McpAuth::redirect_uri(),
            "http://127.0.0.1:19876/mcp/oauth/callback"
        );
    }

    #[test]
    fn mcp_pkce_generates() {
        let pkce = McpAuth::generate_pkce();
        assert!(!pkce.verifier.is_empty());
        assert!(!pkce.challenge.is_empty());
        assert_eq!(pkce.method, "S256");
    }
}
