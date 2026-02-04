//! Security primitives (slim).
//! v0.1.0 retains only path-validation + crypto-key primitives needed by config
//! encryption and request-validation middleware. The sandbox + plugin-verifier
//! + advanced-security stack was deleted with the plugin SDK in v0.1.0. Auth is
//! out of scope (deploy behind a reverse proxy).
//! # Production contract 
//! What's REAL and safe to depend on:
//! - [`path_validator`] — `PathValidator` blocks directory-traversal attacks
//!   on user-supplied paths. Canonicalises inputs + symlink-scopes to the leaf
//!   (last hardened in commit `acbf79a`).
//! - [`key_store`] — `TrustedKeyStore` tracks trusted publisher pubkeys and
//!   revocations for signature verification.
//! - [`key_management`] type surface + real keygen helpers: `KeyType`,
//!   `KeySpecification`, `KeyHandle`, `SecureKey`, `KeyMetadata`,
//!   `ProductionKeyManager`, plus `generate_aes_key` / `generate_rsa_key` /
//!   `generate_ecdsa_key` / `generate_eddsa_key` / `generate_hmac_key`
//!   (real RNG + real crypto primitives — `rand`, `rsa`, `p256`, `ed25519-dalek`).
//! What's TEST-ONLY or stubbed:
//! - `InMemoryKeyEncryption` / `InMemoryKeyStorage` — **gated `#[cfg(test)]`**;
//!   not visible to production binaries. The encryption double returns
//!   plaintext unchanged, which is dangerous if accidentally wired in
//!   production — gating eliminates that risk.
//! - `InMemoryBackupStorage` — `#[deprecated]`; still pub (used as default by
//!   `BackupManager::new()`) but compile-time warnings flag it. `store()` and
//!   `retrieve()` return errors, so misuse fails fast at runtime.
//! - `generate_dh_key` — returns `Err("DH key generation is not supported in
//!   v0.1.0")`. The `KeyType::DH` variant + match arm remain so downstream
//!   callers can signal intent and handle the error.
//! - `KeyType::Custom(_)` — returns the same "not implemented" error pattern.
//! When the cloud-managed Python product ( Pillar 2)
//! ships, the test-only doubles get replaced with HSM/KMS-backed
//! implementations and the `#[cfg(test)]` gates removed.

pub mod key_management;
pub mod key_store;
pub mod path_validator;
pub use key_management::{
    BackupHandle, BackupManager, KeyHandle, KeyHealth, KeyManager, KeyMetadata, KeySpecification,
    KeyState, ProductionKeyManager, RotationPolicy, SecureKey,
};
pub use key_store::TrustedKeyStore;
pub use path_validator::{PathValidationError, PathValidator};
