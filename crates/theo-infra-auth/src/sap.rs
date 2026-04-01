//! SAP AI Core — Service account key authentication.

use crate::error::AuthError;
use crate::store::{AuthEntry, AuthStore};

const PROVIDER_ID: &str = "sap-ai-core";

#[derive(Debug, Clone)]
pub struct SapTokens {
    pub service_key: String,
    pub deployment_id: Option<String>,
    pub resource_group: Option<String>,
}

pub struct SapAiCoreAuth {
    store: AuthStore,
}

impl SapAiCoreAuth {
    pub fn new(store: AuthStore) -> Self {
        Self { store }
    }

    pub fn with_default_store() -> Self {
        Self::new(AuthStore::open())
    }

    pub fn get_tokens(&self) -> Result<Option<SapTokens>, AuthError> {
        let entry = self.store.get(PROVIDER_ID)?;
        match entry {
            Some(AuthEntry::ApiKey { key }) => Ok(Some(SapTokens {
                service_key: key,
                deployment_id: std::env::var("AICORE_DEPLOYMENT_ID").ok(),
                resource_group: std::env::var("AICORE_RESOURCE_GROUP").ok(),
            })),
            None => {
                if let Ok(key) = std::env::var("AICORE_SERVICE_KEY") {
                    Ok(Some(SapTokens {
                        service_key: key,
                        deployment_id: std::env::var("AICORE_DEPLOYMENT_ID").ok(),
                        resource_group: std::env::var("AICORE_RESOURCE_GROUP").ok(),
                    }))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    pub fn has_tokens(&self) -> bool {
        self.get_tokens().ok().flatten().is_some()
    }

    pub fn set_service_key(&self, key: String) -> Result<(), AuthError> {
        self.store.set(PROVIDER_ID, AuthEntry::ApiKey { key })
    }

    pub fn logout(&self) -> Result<(), AuthError> {
        self.store.remove(PROVIDER_ID)
    }

    pub fn provider_id() -> &'static str { PROVIDER_ID }
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
    fn sap_store_and_retrieve() {
        let (store, _dir) = temp_store();
        let auth = SapAiCoreAuth::new(store);
        auth.set_service_key(r#"{"key":"value"}"#.to_string()).unwrap();
        let tokens = auth.get_tokens().unwrap().unwrap();
        assert_eq!(tokens.service_key, r#"{"key":"value"}"#);
    }

    #[test]
    fn sap_logout() {
        let (store, _dir) = temp_store();
        let auth = SapAiCoreAuth::new(store);
        auth.set_service_key("key".to_string()).unwrap();
        assert!(auth.has_tokens());
        auth.logout().unwrap();
        assert!(!auth.has_tokens());
    }

    #[test]
    fn sap_provider_id() {
        assert_eq!(SapAiCoreAuth::provider_id(), "sap-ai-core");
    }
}
