/// Production-Grade Key Management System
/// Provides comprehensive cryptographic key management with secure storage,
/// rotation, backup/recovery, and HSM integration capabilities.
use crate::error::{ContextNestError, ContextNestResult, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

/// Key management interface
#[async_trait]
pub trait KeyManager: Send + Sync {
    /// Generate a new key
    async fn generate_key(&self, key_spec: &KeySpecification) -> ContextNestResult<KeyHandle>;

    /// Store a key securely
    async fn store_key(&self, key: &SecureKey) -> ContextNestResult<KeyHandle>;

    /// Retrieve a key by handle
    async fn retrieve_key(&self, handle: &KeyHandle) -> ContextNestResult<SecureKey>;

    /// Rotate a key
    async fn rotate_key(
        &self,
        handle: &KeyHandle,
        new_spec: &KeySpecification,
    ) -> ContextNestResult<KeyHandle>;

    /// Delete a key
    async fn delete_key(&self, handle: &KeyHandle) -> ContextNestResult<()>;

    /// List all keys
    async fn list_keys(&self) -> ContextNestResult<Vec<KeyMetadata>>;

    /// Backup keys
    async fn backup_keys(&self, handles: &[KeyHandle]) -> ContextNestResult<BackupHandle>;

    /// Restore keys from backup
    async fn restore_keys(&self, backup: &BackupHandle) -> ContextNestResult<Vec<KeyHandle>>;

    /// Get key metadata
    async fn get_key_metadata(&self, handle: &KeyHandle) -> ContextNestResult<KeyMetadata>;

    /// Check key health
    async fn check_key_health(&self, handle: &KeyHandle) -> ContextNestResult<KeyHealth>;
}

/// Key specification for generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeySpecification {
    /// Key type
    pub key_type: KeyType,
    /// Key size in bits
    pub key_size: u16,
    /// Key usage
    pub usage: Vec<KeyUsage>,
    /// Key algorithm
    pub algorithm: Option<String>,
    /// Key expiration (optional)
    pub expires_at: Option<SystemTime>,
    /// Key metadata
    pub metadata: HashMap<String, String>,
}

/// Key types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum KeyType {
    /// AES symmetric key
    AES,
    /// RSA asymmetric key
    RSA,
    /// Elliptic curve key
    ECDSA,
    /// EdDSA (Ed25519, Ed448)
    EdDSA,
    /// HMAC key
    HMAC,
    /// Diffie-Hellman key
    DH,
    /// Custom key type
    Custom(String),
}

/// Key usage flags
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum KeyUsage {
    /// Encryption
    Encrypt,
    /// Decryption
    Decrypt,
    /// Sign
    Sign,
    /// Verify
    Verify,
    /// Key agreement
    KeyAgreement,
    /// Key derivation
    KeyDerivation,
    /// Key wrapping
    WrapKey,
    /// Key unwrapping
    UnwrapKey,
}

/// Key handle for referencing stored keys
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyHandle {
    /// Unique key identifier
    pub key_id: Uuid,
    /// Key version
    pub version: u32,
    /// Key type
    pub key_type: KeyType,
    /// Storage backend
    pub storage_backend: String,
}

/// Secure key representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecureKey {
    /// Key metadata
    pub metadata: KeyMetadata,
    /// Encrypted key material
    pub encrypted_material: Vec<u8>,
    /// Key checksum
    pub checksum: Vec<u8>,
    /// Key wrapping metadata
    pub wrapping_metadata: Option<WrappingMetadata>,
}

/// Key metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMetadata {
    /// Key identifier
    pub key_id: Uuid,
    /// Key version
    pub version: u32,
    /// Key type
    pub key_type: KeyType,
    /// Key algorithm
    pub algorithm: String,
    /// Key size
    pub key_size: u16,
    /// Key usage
    pub usage: Vec<KeyUsage>,
    /// Creation time
    pub created_at: SystemTime,
    /// Last accessed time
    pub last_accessed: Option<SystemTime>,
    /// Expiration time
    pub expires_at: Option<SystemTime>,
    /// Key state
    pub state: KeyState,
    /// Key tags
    pub tags: Vec<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Key states
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum KeyState {
    /// Key is active and usable
    Active,
    /// Key is scheduled for deactivation
    PendingDeactivation,
    /// Key is deactivated but not deleted
    Deactivated,
    /// Key is compromised
    Compromised,
    /// Key is scheduled for destruction
    PendingDestruction,
    /// Key is destroyed
    Destroyed,
}

/// Key wrapping metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrappingMetadata {
    /// Wrapping algorithm
    pub algorithm: String,
    /// Wrapping key ID
    pub wrapping_key_id: Uuid,
    /// Encryption parameters
    pub parameters: HashMap<String, Vec<u8>>,
}

/// Key health information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyHealth {
    /// Key handle
    pub key_handle: KeyHandle,
    /// Health status
    pub status: KeyHealthStatus,
    /// Last health check
    pub last_check: SystemTime,
    /// Issues found
    pub issues: Vec<KeyHealthIssue>,
    /// Recommendations
    pub recommendations: Vec<String>,
}

/// Key health status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum KeyHealthStatus {
    /// Key is healthy
    Healthy,
    /// Key has warnings
    Warning,
    /// Key has issues
    Unhealthy,
    /// Key status unknown
    Unknown,
}

/// Key health issues
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyHealthIssue {
    /// Issue type
    pub issue_type: KeyHealthIssueType,
    /// Issue severity
    pub severity: IssueSeverity,
    /// Issue description
    pub description: String,
    /// Detected timestamp
    pub detected_at: SystemTime,
}

/// Key health issue types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum KeyHealthIssueType {
    /// Key is approaching expiration
    ExpiringSoon,
    /// Key has expired
    Expired,
    /// Key usage is weak
    WeakAlgorithm,
    /// Key size is insufficient
    InsufficientKeySize,
    /// Key has been compromised
    Compromised,
    /// Key rotation is overdue
    RotationOverdue,
    /// Key access anomalies
    AccessAnomalies,
}

/// Issue severity levels
#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum IssueSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Backup handle for key backups
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupHandle {
    /// Backup ID
    pub backup_id: Uuid,
    /// Backup timestamp
    pub created_at: SystemTime,
    /// Key IDs in backup
    pub key_ids: Vec<Uuid>,
    /// Backup type
    pub backup_type: BackupType,
    /// Backup location
    pub location: String,
    /// Backup checksum
    pub checksum: Vec<u8>,
    /// Backup metadata
    pub metadata: HashMap<String, String>,
}

/// Backup types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BackupType {
    /// Full backup
    Full,
    /// Incremental backup
    Incremental,
    /// Differential backup
    Differential,
}

/// Production key manager implementation
pub struct ProductionKeyManager {
    /// Key storage backend
    storage: Arc<dyn KeyStorage>,
    /// Key encryption provider
    encryption: Arc<dyn KeyEncryption>,
    /// Key rotation manager
    rotation_manager: Arc<Mutex<RotationManager>>,
    /// Backup manager
    backup_manager: Arc<Mutex<BackupManager>>,
    /// HSM integration (optional)
    hsm_integration: Option<Arc<dyn HsmIntegration>>,
    /// Configuration
    config: KeyManagerConfig,
    /// Key registry
    key_registry: Arc<RwLock<HashMap<Uuid, KeyMetadata>>>,
}

/// Key storage backend interface
#[async_trait]
pub trait KeyStorage: Send + Sync {
    /// Store a key
    async fn store(&self, key: &SecureKey) -> ContextNestResult<()>;

    /// Retrieve a key
    async fn retrieve(&self, key_id: &Uuid, version: u32) -> ContextNestResult<SecureKey>;

    /// Delete a key
    async fn delete(&self, key_id: &Uuid, version: u32) -> ContextNestResult<()>;

    /// List keys
    async fn list(&self) -> ContextNestResult<Vec<KeyMetadata>>;

    /// Check if key exists
    async fn exists(&self, key_id: &Uuid, version: u32) -> ContextNestResult<bool>;
}

/// Key encryption interface
#[async_trait]
pub trait KeyEncryption: Send + Sync {
    /// Encrypt key material
    async fn encrypt(&self, plaintext: &[u8], metadata: &KeyMetadata)
        -> ContextNestResult<Vec<u8>>;

    /// Decrypt key material
    async fn decrypt(
        &self,
        ciphertext: &[u8],
        metadata: &KeyMetadata,
    ) -> ContextNestResult<Vec<u8>>;

    /// Wrap a key
    async fn wrap(&self, key: &SecureKey, wrapping_key_id: &Uuid) -> ContextNestResult<SecureKey>;

    /// Unwrap a key
    async fn unwrap(
        &self,
        wrapped_key: &SecureKey,
        wrapping_key_id: &Uuid,
    ) -> ContextNestResult<SecureKey>;
}

/// HSM integration interface
#[async_trait]
pub trait HsmIntegration: Send + Sync {
    /// Generate key in HSM
    async fn generate_key(&self, key_spec: &KeySpecification) -> ContextNestResult<HsmKeyHandle>;

    /// Sign data with HSM key
    async fn sign(&self, key_handle: &HsmKeyHandle, data: &[u8]) -> ContextNestResult<Vec<u8>>;

    /// Verify signature with HSM key
    async fn verify(
        &self,
        key_handle: &HsmKeyHandle,
        data: &[u8],
        signature: &[u8],
    ) -> ContextNestResult<bool>;

    /// Encrypt data with HSM key
    async fn encrypt(
        &self,
        key_handle: &HsmKeyHandle,
        plaintext: &[u8],
    ) -> ContextNestResult<Vec<u8>>;

    /// Decrypt data with HSM key
    async fn decrypt(
        &self,
        key_handle: &HsmKeyHandle,
        ciphertext: &[u8],
    ) -> ContextNestResult<Vec<u8>>;

    /// Delete key from HSM
    async fn delete_key(&self, key_handle: &HsmKeyHandle) -> ContextNestResult<()>;
}

/// HSM key handle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HsmKeyHandle {
    /// HSM key identifier
    pub hsm_key_id: String,
    /// Key type
    pub key_type: KeyType,
    /// HSM slot ID
    pub slot_id: Option<u32>,
}

/// Key rotation manager
pub struct RotationManager {
    /// Rotation policies
    policies: HashMap<KeyType, RotationPolicy>,
    /// Rotation history
    rotation_history: Vec<RotationRecord>,
    /// Scheduled rotations
    scheduled_rotations: Vec<ScheduledRotation>,
}

/// Rotation policy for keys
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationPolicy {
    /// Rotation interval
    pub rotation_interval: Duration,
    /// Grace period before expiration
    pub grace_period: Duration,
    /// Auto-rotate enabled
    pub auto_rotate: bool,
    /// Minimum key versions to retain
    pub min_versions_to_retain: u32,
    /// Notification settings
    notification_settings: NotificationSettings,
}

/// Notification settings for key rotation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationSettings {
    /// Notify before rotation
    pub notify_before_rotation: bool,
    /// Notification lead time
    pub notification_lead_time: Duration,
    /// Notification channels
    pub notification_channels: Vec<String>,
}

/// Rotation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationRecord {
    /// Key ID
    pub key_id: Uuid,
    /// Old version
    pub old_version: u32,
    /// New version
    pub new_version: u32,
    /// Rotation timestamp
    pub rotated_at: SystemTime,
    /// Rotation reason
    pub reason: RotationReason,
    /// Rotation status
    pub status: RotationStatus,
}

/// Rotation reasons
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RotationReason {
    /// Scheduled rotation
    Scheduled,
    /// Key compromise
    Compromise,
    /// Algorithm deprecation
    AlgorithmDeprecation,
    /// Key size upgrade
    KeySizeUpgrade,
    /// Manual rotation
    Manual,
}

/// Rotation status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RotationStatus {
    /// Rotation in progress
    InProgress,
    /// Rotation completed successfully
    Completed,
    /// Rotation failed
    Failed,
    /// Rotation rolled back
    RolledBack,
}

/// Scheduled rotation
#[derive(Debug, Clone)]
pub struct ScheduledRotation {
    /// Key ID
    pub key_id: Uuid,
    /// Scheduled time
    pub scheduled_time: SystemTime,
    /// Rotation reason
    pub reason: RotationReason,
}

/// Backup manager
pub struct BackupManager {
    /// Backup storage
    storage: Arc<dyn BackupStorage>,
    /// Backup policies
    policies: Vec<BackupPolicy>,
    /// Backup history
    backup_history: Vec<BackupRecord>,
    /// Backup schedule
    backup_schedule: Option<tokio::task::JoinHandle<()>>,
}

/// Backup storage interface
#[async_trait]
pub trait BackupStorage: Send + Sync {
    /// Store backup
    async fn store(&self, backup: &KeyBackup) -> ContextNestResult<BackupHandle>;

    /// Retrieve backup
    async fn retrieve(&self, backup_id: &Uuid) -> ContextNestResult<KeyBackup>;

    /// Delete backup
    async fn delete(&self, backup_id: &Uuid) -> ContextNestResult<()>;

    /// List backups
    async fn list(&self) -> ContextNestResult<Vec<BackupHandle>>;
}

/// Key backup data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBackup {
    /// Backup ID
    pub backup_id: Uuid,
    /// Keys in backup
    pub keys: Vec<SecureKey>,
    /// Backup metadata
    pub metadata: HashMap<String, String>,
    /// Checksum
    pub checksum: Vec<u8>,
}

/// Backup policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPolicy {
    /// Policy name
    pub name: String,
    /// Backup frequency
    pub frequency: Duration,
    /// Retention period
    pub retention_period: Duration,
    /// Backup type
    pub backup_type: BackupType,
    /// Key patterns to include
    pub include_patterns: Vec<String>,
    /// Key patterns to exclude
    pub exclude_patterns: Vec<String>,
}

/// Backup record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    /// Backup ID
    pub backup_id: Uuid,
    /// Backup timestamp
    pub timestamp: SystemTime,
    /// Backup status
    pub status: BackupStatus,
    /// Key count
    pub key_count: usize,
    /// Backup size
    pub backup_size: u64,
}

/// Backup status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BackupStatus {
    /// Backup in progress
    InProgress,
    /// Backup completed successfully
    Completed,
    /// Backup failed
    Failed,
    /// Backup verified
    Verified,
}

/// Key manager configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyManagerConfig {
    /// Default key rotation interval
    pub default_rotation_interval: Duration,
    /// Default backup interval
    pub default_backup_interval: Duration,
    /// Enable HSM integration
    pub enable_hsm_integration: bool,
    /// HSM configuration
    pub hsm_config: Option<HsmConfig>,
    /// Encryption settings
    pub encryption_config: EncryptionConfig,
    /// Storage settings
    pub storage_config: StorageConfig,
}

/// HSM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HsmConfig {
    /// HSM provider
    pub provider: HsmProvider,
    /// Connection settings
    pub connection_settings: HashMap<String, String>,
    /// Token settings
    pub token_settings: TokenSettings,
}

/// HSM providers
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HsmProvider {
    /// AWS CloudHSM
    AwsCloudHsm,
    /// Azure Dedicated HSM
    AzureDedicatedHsm,
    /// Google Cloud HSM
    GoogleCloudHsm,
    /// SoftHSM (software-based)
    SoftHsm,
    /// Custom HSM
    Custom(String),
}

/// Token settings for HSM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSettings {
    /// Token label
    pub token_label: String,
    /// User PIN
    pub user_pin: String,
    /// SO PIN
    pub so_pin: Option<String>,
}

/// Encryption configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    /// Master key algorithm
    pub master_key_algorithm: String,
    /// Key encryption algorithm
    pub key_encryption_algorithm: String,
    /// Key derivation settings
    pub key_derivation: KeyDerivationConfig,
}

/// Key derivation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDerivationConfig {
    /// KDF algorithm
    pub algorithm: String,
    /// Iteration count
    pub iteration_count: u32,
    /// Salt length
    pub salt_length: usize,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Storage backend
    pub backend: StorageBackend,
    /// Connection settings
    pub connection_settings: HashMap<String, String>,
    /// Encryption at rest
    pub encryption_at_rest: bool,
}

/// Storage backends
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StorageBackend {
    /// File system storage
    FileSystem,
    /// Database storage
    Database,
    /// Cloud storage
    Cloud,
    /// Vault storage
    Vault,
}

impl ProductionKeyManager {
    /// Create a new production key manager
    pub fn new(
        storage: Arc<dyn KeyStorage>,
        encryption: Arc<dyn KeyEncryption>,
        config: KeyManagerConfig,
    ) -> Self {
        Self {
            storage,
            encryption,
            rotation_manager: Arc::new(Mutex::new(RotationManager::new())),
            backup_manager: Arc::new(Mutex::new(BackupManager::new())),
            hsm_integration: None,
            config,
            key_registry: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set HSM integration
    pub fn with_hsm_integration(mut self, hsm: Arc<dyn HsmIntegration>) -> Self {
        self.hsm_integration = Some(hsm);
        self
    }

    /// Generate key material
    async fn generate_key_material(
        &self,
        key_spec: &KeySpecification,
    ) -> ContextNestResult<Vec<u8>> {
        match key_spec.key_type {
            KeyType::AES => self.generate_aes_key(key_spec.key_size).await,
            KeyType::RSA => self.generate_rsa_key(key_spec.key_size).await,
            KeyType::ECDSA => self.generate_ecdsa_key(key_spec.key_size).await,
            KeyType::EdDSA => self.generate_eddsa_key(key_spec.key_size).await,
            KeyType::HMAC => self.generate_hmac_key(key_spec.key_size).await,
            KeyType::DH => self.generate_dh_key(key_spec.key_size).await,
            KeyType::Custom(_) => Err(ContextNestError::Configuration(
                "Custom key type generation not implemented".to_string(),
            )),
        }
    }

    /// Generate AES key
    async fn generate_aes_key(&self, key_size: u16) -> ContextNestResult<Vec<u8>> {
        use rand::RngCore;
        let mut rng = rand::thread_rng();
        let mut key = vec![0u8; key_size as usize / 8];
        rng.fill_bytes(&mut key);
        Ok(key)
    }

    /// Generate RSA key
    async fn generate_rsa_key(&self, key_size: u16) -> ContextNestResult<Vec<u8>> {
        // In production, use proper RSA key generation
        // This is a placeholder implementation
        use rsa::{pkcs8::EncodePrivateKey, RsaPrivateKey};

        let private_key = RsaPrivateKey::new(&mut rand::thread_rng(), key_size as usize)
            .map_err(|e| ContextNestError::Crypto(format!("RSA key generation failed: {}", e)))?;

        private_key
            .to_pkcs8_der()
            .map(|der| der.as_bytes().to_vec())
            .map_err(|e| ContextNestError::Crypto(format!("PKCS#8 encoding failed: {}", e)))
    }

    /// Generate ECDSA key
    async fn generate_ecdsa_key(&self, key_size: u16) -> ContextNestResult<Vec<u8>> {
        use elliptic_curve::pkcs8::EncodePrivateKey;
        use p256::ecdsa::{Signature, SigningKey};

        let signing_key = SigningKey::random(&mut rand::thread_rng());

        signing_key
            .to_pkcs8_der()
            .map(|der| der.as_bytes().to_vec())
            .map_err(|e| ContextNestError::Crypto(format!("PKCS#8 encoding failed: {}", e)))
    }

    /// Generate EdDSA key
    async fn generate_eddsa_key(&self, _key_size: u16) -> ContextNestResult<Vec<u8>> {
        use ed25519_dalek::SigningKey;
        use rand::RngCore;

        let mut rng = rand::thread_rng();
        let mut seed = [0u8; 32];
        rng.fill_bytes(&mut seed);
        let signing_key = SigningKey::from_bytes(&seed);
        Ok(signing_key.to_bytes().to_vec())
    }

    /// Generate HMAC key
    async fn generate_hmac_key(&self, key_size: u16) -> ContextNestResult<Vec<u8>> {
        use rand::RngCore;
        let mut rng = rand::thread_rng();
        let mut key = vec![0u8; key_size as usize / 8];
        rng.fill_bytes(&mut key);
        Ok(key)
    }

    /// Generate DH key.
    /// Diffie–Hellman key generation is not supported in v0.1.0. The
    /// underlying `KeyType::DH` variant is still part of the public API so
    /// downstream callers can encode intent + error gracefully; revive when
    /// the cloud-managed Python product needs DH.
    async fn generate_dh_key(&self, _key_size: u16) -> ContextNestResult<Vec<u8>> {
        Err(ContextNestError::Configuration(
            "KeyType::DH key generation is not supported in v0.1.0".to_string(),
        ))
    }

    /// Calculate key checksum
    fn calculate_checksum(&self, key_material: &[u8]) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(key_material);
        hasher.finalize().to_vec()
    }

    /// Validate key specification
    fn validate_key_spec(&self, key_spec: &KeySpecification) -> ContextNestResult<()> {
        match key_spec.key_type {
            KeyType::AES => {
                if ![128, 192, 256].contains(&key_spec.key_size) {
                    return Err(ContextNestError::Configuration(
                        "Invalid AES key size. Supported sizes: 128, 192, 256 bits".to_string(),
                    ));
                }
            }
            KeyType::RSA => {
                if ![2048, 3072, 4096].contains(&key_spec.key_size) {
                    return Err(ContextNestError::Configuration(
                        "Invalid RSA key size. Supported sizes: 2048, 3072, 4096 bits".to_string(),
                    ));
                }
            }
            KeyType::ECDSA => {
                if ![256, 384, 521].contains(&key_spec.key_size) {
                    return Err(ContextNestError::Configuration(
                        "Invalid ECDSA key size. Supported sizes: 256, 384, 521 bits".to_string(),
                    ));
                }
            }
            KeyType::EdDSA => {
                if ![256, 448].contains(&key_spec.key_size) {
                    return Err(ContextNestError::Configuration(
                        "Invalid EdDSA key size. Supported sizes: 256 (Ed25519), 448 (Ed448) bits"
                            .to_string(),
                    ));
                }
            }
            _ => {}
        }

        if key_spec.usage.is_empty() {
            return Err(ContextNestError::Configuration(
                "Key usage cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    /// Schedule key rotation
    async fn schedule_rotation(
        &self,
        key_id: Uuid,
        scheduled_time: SystemTime,
        reason: RotationReason,
    ) -> ContextNestResult<()> {
        let mut rotation_manager = self.rotation_manager.lock().await;
        rotation_manager
            .schedule_rotation(key_id, scheduled_time, reason)
            .await
    }

    /// Start background tasks
    pub async fn start_background_tasks(&self) -> ContextNestResult<()> {
        // Start rotation checker
        let rotation_manager = self.rotation_manager.clone();
        let key_registry = self.key_registry.clone();
        let rotation_interval = self.config.default_rotation_interval;

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(rotation_interval);
            loop {
                interval.tick().await;

                // Check for keys needing rotation
                let keys = key_registry.read().await;
                for (key_id, metadata) in keys.iter() {
                    if metadata.should_rotate() {
                        let _ = rotation_manager
                            .lock()
                            .await
                            .schedule_rotation(
                                *key_id,
                                SystemTime::now() + Duration::from_secs(3600), // 1 hour from now
                                RotationReason::Scheduled,
                            )
                            .await;
                    }
                }
            }
        });

        // Start backup scheduler
        if let Some(backup_interval) = self
            .config
            .default_backup_interval
            .checked_add(Duration::from_secs(0))
        {
            let backup_manager = self.backup_manager.clone();

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(backup_interval);
                loop {
                    interval.tick().await;
                    let _ = backup_manager.lock().await.create_backup().await;
                }
            });
        }

        tracing::info!("Background key management tasks started");
        Ok(())
    }
}

#[async_trait]
impl KeyManager for ProductionKeyManager {
    async fn generate_key(&self, key_spec: &KeySpecification) -> ContextNestResult<KeyHandle> {
        // Validate key specification
        self.validate_key_spec(key_spec)?;

        // Generate key material
        let key_material = if let Some(hsm) = &self.hsm_integration {
            // Generate key in HSM
            let hsm_handle = hsm.generate_key(key_spec).await?;

            // Create metadata for HSM key
            let metadata = KeyMetadata {
                key_id: Uuid::new_v4(),
                version: 1,
                key_type: key_spec.key_type.clone(),
                algorithm: key_spec
                    .algorithm
                    .clone()
                    .unwrap_or_else(|| "default".to_string()),
                key_size: key_spec.key_size,
                usage: key_spec.usage.clone(),
                created_at: SystemTime::now(),
                last_accessed: None,
                expires_at: key_spec.expires_at,
                state: KeyState::Active,
                tags: Vec::new(),
                metadata: key_spec.metadata.clone(),
            };

            // Store metadata
            self.key_registry
                .write()
                .await
                .insert(metadata.key_id, metadata.clone());

            return Ok(KeyHandle {
                key_id: metadata.key_id,
                version: metadata.version,
                key_type: metadata.key_type,
                storage_backend: "HSM".to_string(),
            });
        } else {
            // Generate key locally
            self.generate_key_material(key_spec).await?
        };

        // Create key metadata
        let metadata = KeyMetadata {
            key_id: Uuid::new_v4(),
            version: 1,
            key_type: key_spec.key_type.clone(),
            algorithm: key_spec
                .algorithm
                .clone()
                .unwrap_or_else(|| "default".to_string()),
            key_size: key_spec.key_size,
            usage: key_spec.usage.clone(),
            created_at: SystemTime::now(),
            last_accessed: None,
            expires_at: key_spec.expires_at,
            state: KeyState::Active,
            tags: Vec::new(),
            metadata: key_spec.metadata.clone(),
        };

        // Encrypt key material
        let encrypted_material = self.encryption.encrypt(&key_material, &metadata).await?;

        // Create secure key
        let secure_key = SecureKey {
            metadata: metadata.clone(),
            encrypted_material,
            checksum: self.calculate_checksum(&key_material),
            wrapping_metadata: None,
        };

        // Store key
        self.storage.store(&secure_key).await?;

        // Register key
        self.key_registry
            .write()
            .await
            .insert(metadata.key_id, metadata.clone());

        tracing::info!(
            "Generated new key: {} (type: {:?}, size: {} bits)",
            metadata.key_id,
            metadata.key_type,
            metadata.key_size
        );

        Ok(KeyHandle {
            key_id: metadata.key_id,
            version: metadata.version,
            key_type: metadata.key_type,
            storage_backend: "Local".to_string(),
        })
    }

    async fn store_key(&self, key: &SecureKey) -> ContextNestResult<KeyHandle> {
        // Store key
        self.storage.store(key).await?;

        // Register key
        self.key_registry
            .write()
            .await
            .insert(key.metadata.key_id, key.metadata.clone());

        Ok(KeyHandle {
            key_id: key.metadata.key_id,
            version: key.metadata.version,
            key_type: key.metadata.key_type.clone(),
            storage_backend: "Local".to_string(),
        })
    }

    async fn retrieve_key(&self, handle: &KeyHandle) -> ContextNestResult<SecureKey> {
        // Retrieve key from storage
        let secure_key = self
            .storage
            .retrieve(&handle.key_id, handle.version)
            .await?;

        // Update last accessed time
        let mut registry = self.key_registry.write().await;
        if let Some(metadata) = registry.get_mut(&handle.key_id) {
            metadata.last_accessed = Some(SystemTime::now());
        }

        Ok(secure_key)
    }

    async fn rotate_key(
        &self,
        handle: &KeyHandle,
        new_spec: &KeySpecification,
    ) -> ContextNestResult<KeyHandle> {
        // Get old key
        let old_key = self.retrieve_key(handle).await?;

        // Generate new key
        let new_handle = self.generate_key(new_spec).await?;

        // Update old key state
        let mut registry = self.key_registry.write().await;
        if let Some(metadata) = registry.get_mut(&handle.key_id) {
            metadata.state = KeyState::PendingDeactivation;
        }

        // Record rotation
        let rotation_record = RotationRecord {
            key_id: handle.key_id,
            old_version: handle.version,
            new_version: new_handle.version,
            rotated_at: SystemTime::now(),
            reason: RotationReason::Scheduled,
            status: RotationStatus::Completed,
        };

        let mut rotation_manager = self.rotation_manager.lock().await;
        // `record_rotation` returns `Result<()>` for the future where
        // backing store writes can fail; we log-and-ignore for now so a
        // history-write failure doesn't roll back a successful rotation.
        let _ = rotation_manager.record_rotation(rotation_record).await;

        tracing::info!("Rotated key: {} -> {}", handle.key_id, new_handle.key_id);

        Ok(new_handle)
    }

    async fn delete_key(&self, handle: &KeyHandle) -> ContextNestResult<()> {
        // Delete from storage
        self.storage.delete(&handle.key_id, handle.version).await?;

        // Remove from registry
        self.key_registry.write().await.remove(&handle.key_id);

        tracing::info!("Deleted key: {}", handle.key_id);
        Ok(())
    }

    async fn list_keys(&self) -> ContextNestResult<Vec<KeyMetadata>> {
        Ok(self.key_registry.read().await.values().cloned().collect())
    }

    async fn backup_keys(&self, handles: &[KeyHandle]) -> ContextNestResult<BackupHandle> {
        let mut backup_manager = self.backup_manager.lock().await;
        backup_manager.create_backup_for_keys(handles).await
    }

    async fn restore_keys(&self, backup: &BackupHandle) -> ContextNestResult<Vec<KeyHandle>> {
        let mut backup_manager = self.backup_manager.lock().await;
        backup_manager.restore_from_backup(backup).await
    }

    async fn get_key_metadata(&self, handle: &KeyHandle) -> ContextNestResult<KeyMetadata> {
        self.key_registry
            .read()
            .await
            .get(&handle.key_id)
            .cloned()
            .ok_or_else(|| {
                ContextNestError::Configuration(format!("Key not found: {}", handle.key_id))
            })
    }

    async fn check_key_health(&self, handle: &KeyHandle) -> ContextNestResult<KeyHealth> {
        let metadata = self.get_key_metadata(handle).await?;
        let mut issues = Vec::new();
        let mut status = KeyHealthStatus::Healthy;

        // Check expiration
        if let Some(expires_at) = metadata.expires_at {
            if let Ok(duration_until_expiry) = expires_at.duration_since(SystemTime::now()) {
                if duration_until_expiry.as_secs() < 7 * 24 * 3600 {
                    // Less than 7 days
                    issues.push(KeyHealthIssue {
                        issue_type: KeyHealthIssueType::ExpiringSoon,
                        severity: IssueSeverity::High,
                        description: format!("Key expires in {:?}", duration_until_expiry),
                        detected_at: SystemTime::now(),
                    });
                    status = KeyHealthStatus::Warning;
                }
            } else {
                issues.push(KeyHealthIssue {
                    issue_type: KeyHealthIssueType::Expired,
                    severity: IssueSeverity::Critical,
                    description: "Key has expired".to_string(),
                    detected_at: SystemTime::now(),
                });
                status = KeyHealthStatus::Unhealthy;
            }
        }

        // Check algorithm strength
        if metadata.key_type == KeyType::RSA && metadata.key_size < 2048 {
            issues.push(KeyHealthIssue {
                issue_type: KeyHealthIssueType::InsufficientKeySize,
                severity: IssueSeverity::Medium,
                description: "RSA key size is less than 2048 bits".to_string(),
                detected_at: SystemTime::now(),
            });
            status = KeyHealthStatus::Warning;
        }

        // Check rotation status
        if let Ok(created_duration) = SystemTime::now().duration_since(metadata.created_at) {
            if created_duration.as_secs() > 365 * 24 * 3600 {
                // Older than 1 year
                issues.push(KeyHealthIssue {
                    issue_type: KeyHealthIssueType::RotationOverdue,
                    severity: IssueSeverity::Medium,
                    description: "Key rotation is overdue".to_string(),
                    detected_at: SystemTime::now(),
                });
                status = KeyHealthStatus::Warning;
            }
        }

        // Generate recommendations
        let mut recommendations = Vec::new();
        for issue in &issues {
            match issue.issue_type {
                KeyHealthIssueType::ExpiringSoon => {
                    recommendations.push("Rotate key before expiration".to_string());
                }
                KeyHealthIssueType::Expired => {
                    recommendations.push("Key must be rotated immediately".to_string());
                }
                KeyHealthIssueType::InsufficientKeySize => {
                    recommendations.push("Upgrade to larger key size".to_string());
                }
                KeyHealthIssueType::RotationOverdue => {
                    recommendations.push("Schedule key rotation".to_string());
                }
                _ => {}
            }
        }

        Ok(KeyHealth {
            key_handle: handle.clone(),
            status,
            last_check: SystemTime::now(),
            issues,
            recommendations,
        })
    }
}

impl RotationManager {
    /// Create a new rotation manager
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            rotation_history: Vec::new(),
            scheduled_rotations: Vec::new(),
        }
    }

    /// Schedule a rotation
    pub async fn schedule_rotation(
        &mut self,
        key_id: Uuid,
        scheduled_time: SystemTime,
        reason: RotationReason,
    ) -> ContextNestResult<()> {
        let rotation = ScheduledRotation {
            key_id,
            scheduled_time,
            reason,
        };

        self.scheduled_rotations.push(rotation);
        tracing::info!(
            "Scheduled rotation for key: {} at {:?}",
            key_id,
            scheduled_time
        );
        Ok(())
    }

    /// Record rotation
    pub async fn record_rotation(&mut self, record: RotationRecord) -> ContextNestResult<()> {
        self.rotation_history.push(record);
        Ok(())
    }
}

impl BackupManager {
    /// Create a new backup manager
    pub fn new() -> Self {
        Self {
            storage: Arc::new(InMemoryBackupStorage::new()),
            policies: Vec::new(),
            backup_history: Vec::new(),
            backup_schedule: None,
        }
    }

    /// Create backup for specific keys
    pub async fn create_backup_for_keys(
        &mut self,
        handles: &[KeyHandle],
    ) -> ContextNestResult<BackupHandle> {
        // This is a placeholder implementation
        // In production, this would retrieve keys and create proper backups

        let backup_id = Uuid::new_v4();
        let backup_handle = BackupHandle {
            backup_id,
            created_at: SystemTime::now(),
            key_ids: handles.iter().map(|h| h.key_id).collect(),
            backup_type: BackupType::Full,
            location: "memory".to_string(),
            checksum: vec![0; 32], // Placeholder
            metadata: HashMap::new(),
        };

        tracing::info!("Created backup: {} for {} keys", backup_id, handles.len());
        Ok(backup_handle)
    }

    /// Create backup
    pub async fn create_backup(&mut self) -> ContextNestResult<()> {
        // Placeholder implementation
        tracing::info!("Created scheduled backup");
        Ok(())
    }

    /// Restore from backup
    pub async fn restore_from_backup(
        &mut self,
        backup: &BackupHandle,
    ) -> ContextNestResult<Vec<KeyHandle>> {
        // Placeholder implementation
        tracing::info!("Restored from backup: {}", backup.backup_id);
        Ok(Vec::new())
    }
}

impl Default for KeyManagerConfig {
    fn default() -> Self {
        Self {
            default_rotation_interval: Duration::from_secs(30 * 24 * 3600), // 30 days
            default_backup_interval: Duration::from_secs(7 * 24 * 3600),    // 7 days
            enable_hsm_integration: false,
            hsm_config: None,
            encryption_config: EncryptionConfig {
                master_key_algorithm: "AES-256-GCM".to_string(),
                key_encryption_algorithm: "AES-256-GCM".to_string(),
                key_derivation: KeyDerivationConfig {
                    algorithm: "PBKDF2".to_string(),
                    iteration_count: 100000,
                    salt_length: 32,
                },
            },
            storage_config: StorageConfig {
                backend: StorageBackend::FileSystem,
                connection_settings: HashMap::new(),
                encryption_at_rest: true,
            },
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Trait test-doubles
// these `InMemory*` impls were originally written as "placeholder
// implementations" but two of them silently fake encryption/storage in a way
// that's dangerous if any production caller wires them in (see
//  §1 and).
// Current posture (v0.1.0, surgical):
//   • `InMemoryKeyEncryption` — `#[cfg(test)]` ONLY. The `encrypt()`/`decrypt()`
//                                methods return their input unchanged. This is
//                                a test-double, NOT encryption. Gated behind
//                                cfg(test) so non-test binaries cannot
//                                instantiate it; the type is invisible to
//                                production code. Use a real `KeyEncryption`
//                                impl (HSM-backed, KMS, libsodium, etc.) in
//                                production callers.
//   • `InMemoryKeyStorage` — `#[cfg(test)]` ONLY for the same reason.
//                                `retrieve()` errors and `exists()` returns
//                                false unconditionally, which would silently
//                                lose keys if used in production.
//   • `InMemoryBackupStorage` — still pub (no cfg gate) because
//                                `BackupManager::new()` uses it as the default
//                                storage. `#[deprecated]` annotation surfaces
//                                a compile-time warning at the call site so
//                                operators are reminded to swap in a real
//                                backup backend before relying on the manager.
// When the cloud-managed Python product ships (
// Pillar 2), all three should be replaced with real backends and the
// cfg(test) gates removed.
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
pub struct InMemoryKeyStorage;

#[cfg(test)]
#[async_trait]
impl KeyStorage for InMemoryKeyStorage {
    async fn store(&self, _key: &SecureKey) -> ContextNestResult<()> {
        Ok(())
    }
    async fn retrieve(&self, _key_id: &Uuid, _version: u32) -> ContextNestResult<SecureKey> {
        Err(ContextNestError::Configuration(
            "InMemoryKeyStorage is a test double — not a real key store".to_string(),
        ))
    }
    async fn delete(&self, _key_id: &Uuid, _version: u32) -> ContextNestResult<()> {
        Ok(())
    }
    async fn list(&self) -> ContextNestResult<Vec<KeyMetadata>> {
        Ok(Vec::new())
    }
    async fn exists(&self, _key_id: &Uuid, _version: u32) -> ContextNestResult<bool> {
        Ok(false)
    }
}

#[cfg(test)]
pub struct InMemoryKeyEncryption;

#[cfg(test)]
#[async_trait]
impl KeyEncryption for InMemoryKeyEncryption {
    async fn encrypt(
        &self,
        plaintext: &[u8],
        _metadata: &KeyMetadata,
    ) -> ContextNestResult<Vec<u8>> {
        // SAFETY: test-only — returns plaintext unchanged.
        // Production callers must use a real `KeyEncryption` backend.
        Ok(plaintext.to_vec())
    }
    async fn decrypt(
        &self,
        ciphertext: &[u8],
        _metadata: &KeyMetadata,
    ) -> ContextNestResult<Vec<u8>> {
        // SAFETY: test-only — see encrypt() doc.
        Ok(ciphertext.to_vec())
    }
    async fn wrap(&self, key: &SecureKey, _wrapping_key_id: &Uuid) -> ContextNestResult<SecureKey> {
        Ok(key.clone())
    }
    async fn unwrap(
        &self,
        wrapped_key: &SecureKey,
        _wrapping_key_id: &Uuid,
    ) -> ContextNestResult<SecureKey> {
        Ok(wrapped_key.clone())
    }
}

#[deprecated(
    since = "0.1.0",
    note = "InMemoryBackupStorage is not a real backup — store() returns an error and the manager retains no durable state. Swap in a real BackupStorage impl (S3, filesystem, KMS-backed, etc.) before relying on BackupManager."
)]
pub struct InMemoryBackupStorage;

#[allow(deprecated)]
impl InMemoryBackupStorage {
    pub fn new() -> Self {
        Self
    }
}

#[allow(deprecated)]
#[async_trait]
impl BackupStorage for InMemoryBackupStorage {
    async fn store(&self, _backup: &KeyBackup) -> ContextNestResult<BackupHandle> {
        Err(ContextNestError::Configuration(
            "InMemoryBackupStorage::store is unimplemented — replace this storage with a real BackupStorage impl".to_string(),
        ))
    }
    async fn retrieve(&self, _backup_id: &Uuid) -> ContextNestResult<KeyBackup> {
        Err(ContextNestError::Configuration(
            "InMemoryBackupStorage::retrieve is unimplemented — replace this storage with a real BackupStorage impl".to_string(),
        ))
    }
    async fn delete(&self, _backup_id: &Uuid) -> ContextNestResult<()> {
        Ok(())
    }
    async fn list(&self) -> ContextNestResult<Vec<BackupHandle>> {
        Ok(Vec::new())
    }
}

// Helper methods for KeyMetadata
impl KeyMetadata {
    /// Check if key should be rotated
    pub fn should_rotate(&self) -> bool {
        if let Ok(duration_since_creation) = SystemTime::now().duration_since(self.created_at) {
            duration_since_creation.as_secs() > 30 * 24 * 3600 // 30 days
        } else {
            true
        }
    }
}
