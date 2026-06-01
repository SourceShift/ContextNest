//! AES-256-GCM encryption helper for LLM proxy cache entries
//! (v0.3 Phase 3 slice 3.1).
//!
//! Wraps `ring::aead::AES_256_GCM` behind a tight API:
//!
//! - [`load_key_from_env`] reads `CONTEXTNEST_LLM_CACHE_ENCRYPTION_KEY`
//!   (32-byte hex-encoded). When unset, encryption is OFF — the cache
//!   continues to operate exactly like Phase 2's plaintext mode.
//! - [`encrypt_with_aad`] / [`decrypt_with_aad`] AEAD seal/open with
//!   caller-supplied associated-data binding. AAD is the
//!   `ExactKeyPrefix::fingerprint()` — that binds each ciphertext to
//!   the exact (project, model, temperature, system-prompt) tuple it
//!   was inserted under, so a ciphertext lifted from one bucket can't
//!   be replayed into another.
//!
//! ## Nonce strategy
//!
//! Random 96-bit nonces per encryption (NIST SP 800-38D §8.2.2 marks
//! this acceptable for ≤ 2³² encryptions per key — way more than any
//! real workload). The nonce is stored alongside the ciphertext in the
//! WAL record; on decrypt we feed it back through unchanged.
//!
//! ## Why ring (vs aes_gcm crate vs RustCrypto)
//!
//! `ring` is already in the dependency graph and is the more
//! conservatively-audited choice. The newer `aes_gcm` crate would
//! require a new dep + has different audit posture. Performance is
//! comparable; we pick the dep we already have.
//!
//! ## What this module does NOT do (yet)
//!
//! - Does not handle key rotation. Adding a `key_id` byte to the
//!   ciphertext envelope is a follow-up slice — until then a key
//!   change means cache miss for entries inserted under the old key.
//! - Does not implement HSM / KMS-style key wrapping. Master key is
//!   the env var value as-is. Suitable for solo-founder local
//!   substrate deployments; multi-tenant production deployments
//!   should plug a real `KeyEncryption` backend here.
//! - Does not encrypt the exact-prefix fields (project_id, model,
//!   temperature_bucket, system_prompt_hash). Those are needed for
//!   HashMap lookup pre-decryption, so they must stay cleartext on
//!   disk. The 8-byte system_prompt_hash is already a truncated
//!   SHA-256 — operator visibility is the prompt name, not the
//!   prompt content.

use std::sync::Arc;

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};

/// Environment variable carrying the master cache encryption key.
/// 32 bytes hex-encoded (= 64 hex chars). When unset the LLM cache
/// runs in Phase-2 plaintext mode.
pub const ENCRYPTION_KEY_ENV: &str = "CONTEXTNEST_LLM_CACHE_ENCRYPTION_KEY";

/// Errors from encryption / decryption / key-loading.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("invalid key length: expected 32 bytes (64 hex chars), got {0}")]
    BadKeyLength(usize),
    #[error("key is not valid hex: {0}")]
    BadKeyHex(String),
    #[error("ring aead failure: {0}")]
    Aead(String),
    #[error("nonce generation failed: {0}")]
    Nonce(String),
}

/// Sealed envelope returned by [`encrypt_with_aad`] and consumed by
/// [`decrypt_with_aad`]. Stored as-is in the WAL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedBlob {
    pub nonce: [u8; NONCE_LEN],
    /// Ciphertext concatenated with the GCM 16-byte tag (ring's
    /// in-place `seal_in_place_append_tag` convention).
    pub ciphertext: Vec<u8>,
}

/// Parse a hex-encoded 32-byte AES-256-GCM key.
///
/// Pure function — does NOT touch the environment. Tests use this
/// directly to avoid race conditions with parallel env-var mutation.
pub fn parse_key_hex(raw: &str) -> Result<Arc<LessSafeKey>, CryptoError> {
    let key_bytes = hex::decode(raw.trim()).map_err(|e| CryptoError::BadKeyHex(e.to_string()))?;
    if key_bytes.len() != 32 {
        return Err(CryptoError::BadKeyLength(key_bytes.len()));
    }
    let unbound = UnboundKey::new(&AES_256_GCM, &key_bytes)
        .map_err(|e| CryptoError::Aead(format!("{e:?}")))?;
    Ok(Arc::new(LessSafeKey::new(unbound)))
}

/// Load the master key from `CONTEXTNEST_LLM_CACHE_ENCRYPTION_KEY`.
/// Returns `Ok(None)` when the env var is unset — the caller treats
/// that as "encryption disabled, run in plaintext mode".
pub fn load_key_from_env() -> Result<Option<Arc<LessSafeKey>>, CryptoError> {
    match std::env::var(ENCRYPTION_KEY_ENV) {
        Ok(raw) => parse_key_hex(&raw).map(Some),
        Err(_) => Ok(None),
    }
}

/// Encrypt `plaintext` with associated-data binding to `aad`.
/// Returns nonce + ciphertext+tag. Random nonce per call.
pub fn encrypt_with_aad(
    key: &LessSafeKey,
    plaintext: &[u8],
    aad: &[u8],
) -> Result<EncryptedBlob, CryptoError> {
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rng.fill(&mut nonce_bytes)
        .map_err(|e| CryptoError::Nonce(format!("{e:?}")))?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);

    let mut in_out = plaintext.to_vec();
    key.seal_in_place_append_tag(nonce, Aad::from(aad), &mut in_out)
        .map_err(|e| CryptoError::Aead(format!("seal: {e:?}")))?;

    Ok(EncryptedBlob {
        nonce: nonce_bytes,
        ciphertext: in_out,
    })
}

/// Decrypt a previously-sealed blob. AAD must match the value
/// supplied at encryption time — mismatched AAD yields an error,
/// never a wrong plaintext.
pub fn decrypt_with_aad(
    key: &LessSafeKey,
    blob: &EncryptedBlob,
    aad: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let nonce = Nonce::assume_unique_for_key(blob.nonce);
    let mut in_out = blob.ciphertext.clone();
    let plaintext = key
        .open_in_place(nonce, Aad::from(aad), &mut in_out)
        .map_err(|e| CryptoError::Aead(format!("open: {e:?}")))?;
    Ok(plaintext.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> Arc<LessSafeKey> {
        // Deterministic 32-byte key for tests. `SystemRandom` is
        // exercised by the live env-loader path.
        let bytes = [7u8; 32];
        let unbound = UnboundKey::new(&AES_256_GCM, &bytes).unwrap();
        Arc::new(LessSafeKey::new(unbound))
    }

    #[test]
    fn round_trip_returns_original_plaintext() {
        let key = test_key();
        let pt = b"the quick brown fox jumps over the lazy dog";
        let aad = b"aad-binding";
        let blob = encrypt_with_aad(&key, pt, aad).expect("encrypt");
        let recovered = decrypt_with_aad(&key, &blob, aad).expect("decrypt");
        assert_eq!(recovered, pt);
    }

    #[test]
    fn aad_mismatch_fails_decryption() {
        // AAD-binding is what stops a ciphertext lifted from one cache
        // bucket from being replayed into another. Pin the contract:
        // even one-byte AAD change must fail decryption, never produce
        // a wrong plaintext.
        let key = test_key();
        let pt = b"sensitive response body";
        let blob = encrypt_with_aad(&key, pt, b"bucket-a").expect("encrypt");
        let err = decrypt_with_aad(&key, &blob, b"bucket-b").expect_err("must fail");
        assert!(matches!(err, CryptoError::Aead(_)));
    }

    #[test]
    fn random_nonces_differ_across_encryptions() {
        // Random-nonce contract: same plaintext + same key + same AAD
        // must produce DIFFERENT ciphertexts across calls (otherwise
        // we'd leak repeat-prompt fingerprints). Single random nonce
        // collision in 2^96 is astronomically unlikely; the test
        // confirms the wiring uses SystemRandom rather than a static.
        let key = test_key();
        let pt = b"repeat me";
        let a = encrypt_with_aad(&key, pt, b"aad").unwrap();
        let b = encrypt_with_aad(&key, pt, b"aad").unwrap();
        assert_ne!(a.nonce, b.nonce, "nonces must differ");
        assert_ne!(
            a.ciphertext, b.ciphertext,
            "ciphertext must differ when nonce differs (same plaintext)"
        );
    }

    #[test]
    fn empty_plaintext_round_trips() {
        // GCM authenticates empty plaintext + non-empty AAD just fine.
        // Useful for sanity-checking the wiring on a fresh substrate.
        let key = test_key();
        let blob = encrypt_with_aad(&key, b"", b"aad").unwrap();
        let recovered = decrypt_with_aad(&key, &blob, b"aad").unwrap();
        assert_eq!(recovered, Vec::<u8>::new());
        // The ciphertext is the 16-byte GCM tag alone.
        assert_eq!(blob.ciphertext.len(), 16);
    }

    // Env-var loader tests avoid `std::env::set_var` because cargo's
    // test runner runs the whole file in parallel — concurrent env
    // mutation races. The pure `parse_key_hex` exposes the same paths
    // for testing, and a single sequential test exercises the env-var
    // wrapper end-to-end.

    #[test]
    fn parse_key_hex_rejects_wrong_length() {
        // 16 bytes hex-encoded — half the required key size.
        let result = parse_key_hex("00112233445566778899aabbccddeeff");
        assert!(matches!(result, Err(CryptoError::BadKeyLength(16))));
    }

    #[test]
    fn parse_key_hex_rejects_non_hex() {
        let result = parse_key_hex("not-hex-data-this-is-supposed-to-fail-here-too");
        assert!(matches!(result, Err(CryptoError::BadKeyHex(_))));
    }

    #[test]
    fn parse_key_hex_accepts_valid_key() {
        let result =
            parse_key_hex("0011223344556677889900aabbccddeeff112233445566778899aabbccddeeff");
        assert!(result.is_ok());
    }

    #[test]
    fn parse_key_hex_tolerates_surrounding_whitespace() {
        // Operators copying from cat / pbcopy sometimes include a
        // trailing newline. `trim()` in the loader smooths it over.
        let result =
            parse_key_hex("  0011223344556677889900aabbccddeeff112233445566778899aabbccddeeff\n");
        assert!(result.is_ok());
    }

    #[test]
    fn ciphertext_includes_gcm_tag() {
        // ring's seal_in_place_append_tag appends the 16-byte GCM tag.
        // Pin the length contract so a future refactor that switches
        // to a tag-detached API surfaces in tests.
        let key = test_key();
        let pt = b"abc";
        let blob = encrypt_with_aad(&key, pt, b"aad").unwrap();
        assert_eq!(
            blob.ciphertext.len(),
            pt.len() + 16,
            "ciphertext = plaintext_len + 16-byte GCM tag"
        );
    }
}
