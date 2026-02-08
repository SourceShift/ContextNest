//! Trusted Key Store - CRITICAL component for signature verification
//! Manages trusted publisher public keys and key revocation.
//! This is the foundation of the plugin trust model.

use std::collections::HashMap;

/// TrustedKeyStore manages trusted publisher keys and revocations
pub struct TrustedKeyStore {
    /// Publisher name → Public key mapping
    publishers: HashMap<String, Vec<u8>>,

    /// Revoked keys (key bytes → revocation reason)
    revoked_keys: HashMap<Vec<u8>, String>,
}

impl TrustedKeyStore {
    /// Create a new empty TrustedKeyStore
    pub fn new() -> Self {
        Self {
            publishers: HashMap::new(),
            revoked_keys: HashMap::new(),
        }
    }

    /// Create a TrustedKeyStore with default trusted publishers
    pub fn with_defaults() -> Self {
        let mut store = Self::new();

        // Add default trusted publishers here
        // In production, these would be loaded from a secure configuration
        tracing::info!("Initialized TrustedKeyStore with default publishers");

        store
    }

    /// Check if a public key is trusted for a given publisher
    pub fn is_trusted(&self, public_key: &[u8], publisher: &str) -> bool {
        if let Some(trusted_key) = self.publishers.get(publisher) {
            trusted_key == public_key
        } else {
            false
        }
    }

    /// Check if a public key has been revoked
    pub fn is_revoked(&self, public_key: &[u8]) -> bool {
        self.revoked_keys.contains_key(public_key)
    }

    /// Add a trusted publisher with their public key
    /// # Arguments
    /// * `publisher` - Publisher name (e.g., "Acme Corp")
    /// * `public_key` - Ed25519 public key (32 bytes)
    pub fn add_trusted_publisher(&mut self, publisher: String, public_key: Vec<u8>) {
        if public_key.len() != ed25519_dalek::PUBLIC_KEY_LENGTH {
            tracing::warn!(
                "Invalid public key length for {}: expected {}, got {}",
                publisher,
                ed25519_dalek::PUBLIC_KEY_LENGTH,
                public_key.len()
            );
            return;
        }

        tracing::info!("Added trusted publisher: {}", publisher);
        self.publishers.insert(publisher, public_key);
    }

    /// Revoke a public key
    /// # Arguments
    /// * `public_key` - Key to revoke
    /// * `reason` - Reason for revocation (for audit trail)
    pub fn revoke_key(&mut self, public_key: Vec<u8>, reason: String) {
        tracing::warn!(
            "Revoking public key (reason: {}): {}",
            reason,
            hex::encode(&public_key)
        );
        self.revoked_keys.insert(public_key, reason);
    }

    /// Remove a publisher from the trusted list
    pub fn remove_publisher(&mut self, publisher: &str) -> Option<Vec<u8>> {
        tracing::warn!("Removing publisher from trusted list: {}", publisher);
        self.publishers.remove(publisher)
    }

    /// Get all trusted publishers
    pub fn get_publishers(&self) -> Vec<String> {
        self.publishers.keys().cloned().collect()
    }

    /// Get revocation reason for a key
    pub fn get_revocation_reason(&self, public_key: &[u8]) -> Option<&str> {
        self.revoked_keys.get(public_key).map(|s| s.as_str())
    }

    /// Get count of trusted publishers
    pub fn publisher_count(&self) -> usize {
        self.publishers.len()
    }

    /// Get count of revoked keys
    pub fn revoked_count(&self) -> usize {
        self.revoked_keys.len()
    }
}

impl Default for TrustedKeyStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_check_trusted_publisher() {
        let mut store = TrustedKeyStore::new();
        let public_key = vec![0u8; 32]; // Valid Ed25519 key length

        store.add_trusted_publisher("Acme Corp".to_string(), public_key.clone());

        assert!(store.is_trusted(&public_key, "Acme Corp"));
        assert!(!store.is_trusted(&public_key, "Other Corp"));
    }

    #[test]
    fn test_key_revocation() {
        let mut store = TrustedKeyStore::new();
        let public_key = vec![0u8; 32];

        store.add_trusted_publisher("Acme Corp".to_string(), public_key.clone());
        assert!(!store.is_revoked(&public_key));

        store.revoke_key(public_key.clone(), "Security breach".to_string());
        assert!(store.is_revoked(&public_key));

        let reason = store.get_revocation_reason(&public_key);
        assert_eq!(reason, Some("Security breach"));
    }

    #[test]
    fn test_remove_publisher() {
        let mut store = TrustedKeyStore::new();
        let public_key = vec![0u8; 32];

        store.add_trusted_publisher("Acme Corp".to_string(), public_key.clone());
        assert_eq!(store.publisher_count(), 1);

        let removed_key = store.remove_publisher("Acme Corp");
        assert_eq!(removed_key, Some(public_key.clone()));
        assert_eq!(store.publisher_count(), 0);
        assert!(!store.is_trusted(&public_key, "Acme Corp"));
    }

    #[test]
    fn test_get_publishers() {
        let mut store = TrustedKeyStore::new();

        store.add_trusted_publisher("Acme Corp".to_string(), vec![0u8; 32]);
        store.add_trusted_publisher("Beta Inc".to_string(), vec![1u8; 32]);

        let publishers = store.get_publishers();
        assert_eq!(publishers.len(), 2);
        assert!(publishers.contains(&"Acme Corp".to_string()));
        assert!(publishers.contains(&"Beta Inc".to_string()));
    }

    #[test]
    fn test_invalid_key_length_rejected() {
        let mut store = TrustedKeyStore::new();
        let invalid_key = vec![0u8; 16]; // Wrong length

        store.add_trusted_publisher("Acme Corp".to_string(), invalid_key.clone());

        // Should not be added due to invalid length
        assert_eq!(store.publisher_count(), 0);
    }

    #[test]
    fn test_counts() {
        let mut store = TrustedKeyStore::new();

        assert_eq!(store.publisher_count(), 0);
        assert_eq!(store.revoked_count(), 0);

        store.add_trusted_publisher("Acme".to_string(), vec![0u8; 32]);
        assert_eq!(store.publisher_count(), 1);

        store.revoke_key(vec![1u8; 32], "Test".to_string());
        assert_eq!(store.revoked_count(), 1);
    }
}
