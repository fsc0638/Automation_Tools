//! Low-level vault cryptographic primitives.
//!
//! Pure synchronous functions — no DB, no I/O, no async.  All vault
//! operations ultimately reduce to calls here, making this the single
//! place to audit, test, and reason about the cryptographic correctness
//! of the system.
//!
//! ## Key hierarchy (Option B — User KEK + System Recovery)
//!
//! ```text
//! Login:
//!   Argon2id(password, users.kek_salt) ──► User KEK   (RAM only, never persisted)
//!
//! Seal an object:
//!   OsRng ──► random DEK (32 bytes)
//!   DEK ──AES-256-GCM──► ciphertext          stored in vault_ciphertexts
//!   DEK ──wrap(User KEK)──► wrapped_dek       stored in vault_key_wrappings (alias=user:<id>)
//!   DEK ──wrap(System KEK)──► wrapped_dek     stored in vault_key_wrappings (alias=system_v1)
//!
//! Open an object (normal path):
//!   vault_key_wrappings[user:<id>].wrapped_dek
//!     ──unwrap(User KEK)──► DEK
//!     ──AES-256-GCM──► plaintext
//!
//! Open an object (admin recovery path, CLI only):
//!   vault_key_wrappings[system_v1].wrapped_dek
//!     ──unwrap(System KEK)──► DEK
//!     ──AES-256-GCM──► plaintext   +  high-visibility audit log entry
//! ```

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore, Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;

// ── Argon2id parameters for User KEK derivation ──────────────────────
// 64 MiB memory, 3 iterations, 4 lanes → ~200 ms on modern hardware.
// This runs exactly once per login — intentional slowness is the point.
const ARGON2_M_COST: u32 = 65_536; // KiB (= 64 MiB)
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 4;

// Domain separator keeps KEK derivation distinct from the password-hash
// path in auth.rs so the two Argon2 outputs can never be confused.
const KEK_DOMAIN: &[u8] = b"kway-kek-v1::";

// ── Core primitives ───────────────────────────────────────────────────

/// Generate a cryptographically random 32-byte Data Encryption Key.
pub fn generate_dek() -> [u8; 32] {
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    key
}

/// Encrypt `plaintext` with `dek` under the given `aad`.
///
/// `aad` must be `"{object_type}:{object_id}"`.  It is authenticated but
/// not encrypted — wrong AAD causes decryption to fail (anti-substitution).
///
/// Returns `(nonce[12], ciphertext_with_tag)`.
pub fn seal(dek: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Result<([u8; 12], Vec<u8>)> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(dek));
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher
        .encrypt(
            &nonce,
            aes_gcm::aead::Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|e| anyhow!("vault seal: {e}"))?;
    Ok((nonce.into(), ct))
}

/// Decrypt `ciphertext` using `dek`, `nonce`, and the matching `aad`.
///
/// Returns `Err` on any authentication failure (wrong key, wrong nonce,
/// wrong AAD, or corrupt ciphertext).
pub fn open(dek: &[u8; 32], nonce: &[u8; 12], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(dek));
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            aes_gcm::aead::Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|e| anyhow!("vault open (auth tag mismatch — wrong key or corrupt data): {e}"))
}

/// Wrap a DEK using `cipher` (either a User KEK or the System KEK).
///
/// Internally: base64-encodes the raw DEK bytes, then calls
/// `TokenCipher::encrypt` which applies AES-256-GCM with a fresh nonce.
/// The result is a base64 string safe to store in `vault_key_wrappings`.
pub fn wrap_dek(cipher: &crate::crypto::TokenCipher, dek: &[u8; 32]) -> Result<String> {
    let dek_b64 = B64.encode(dek);
    cipher.encrypt(&dek_b64)
}

/// Unwrap a DEK that was previously produced by `wrap_dek`.
/// Returns `Err` if the cipher key does not match or the ciphertext is corrupt.
pub fn unwrap_dek(cipher: &crate::crypto::TokenCipher, wrapped: &str) -> Result<[u8; 32]> {
    let dek_b64 = cipher.decrypt(wrapped)?;
    let bytes = B64
        .decode(&dek_b64)
        .map_err(|e| anyhow!("unwrap_dek: invalid base64: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("unwrap_dek: expected 32 bytes after decode"))
}

// ── User KEK derivation ───────────────────────────────────────────────

/// Derive a 32-byte User KEK from the user's plaintext password and their
/// stable per-user `kek_salt` (stored in `users.kek_salt`).
///
/// Properties:
/// - Deterministic: same (password, salt) → same KEK, always.
/// - Domain-separated from the login password-hash via `KEK_DOMAIN`.
/// - Slow by design (~200 ms); call exactly once at login, never per-request.
///
/// The result should be immediately wrapped in `TokenCipher::from_raw_key`
/// and stored in `AppState::session_keys` for the duration of the session.
pub fn derive_user_kek(password: &str, kek_salt: &[u8]) -> Result<[u8; 32]> {
    let params =
        Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32))
            .map_err(|e| anyhow!("argon2 params: {e}"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    // Prepend domain separator before hashing.
    let mut input = KEK_DOMAIN.to_vec();
    input.extend_from_slice(password.as_bytes());

    let mut kek = [0u8; 32];
    argon2
        .hash_password_into(&input, kek_salt, &mut kek)
        .map_err(|e| anyhow!("derive_user_kek: {e}"))?;
    Ok(kek)
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::TokenCipher;
    use std::collections::HashSet;

    // ── seal / open ──────────────────────────────────────────────────

    #[test]
    fn roundtrip_small() {
        let dek = generate_dek();
        let aad = b"vault_secret:00000000-0000-0000-0000-000000000001";
        let plain = b"hunter2";
        let (nonce, ct) = seal(&dek, plain, aad).unwrap();
        assert_eq!(open(&dek, &nonce, &ct, aad).unwrap(), plain);
    }

    #[test]
    fn roundtrip_large() {
        let dek = generate_dek();
        let aad = b"vault_file:aaaabbbbcccc";
        let plain = vec![0xABu8; 1_024 * 1_024]; // 1 MiB
        let (nonce, ct) = seal(&dek, &plain, aad).unwrap();
        assert_eq!(open(&dek, &nonce, &ct, aad).unwrap(), plain);
    }

    #[test]
    fn wrong_dek_fails() {
        let dek1 = generate_dek();
        let dek2 = generate_dek();
        let aad = b"vault_secret:x";
        let (nonce, ct) = seal(&dek1, b"secret", aad).unwrap();
        assert!(open(&dek2, &nonce, &ct, aad).is_err());
    }

    #[test]
    fn wrong_aad_fails() {
        let dek = generate_dek();
        let (nonce, ct) = seal(&dek, b"secret", b"vault_secret:abc").unwrap();
        assert!(
            open(&dek, &nonce, &ct, b"vault_secret:DIFFERENT").is_err(),
            "AAD mismatch must fail authentication"
        );
    }

    #[test]
    fn generate_dek_uniqueness() {
        let mut seen = HashSet::new();
        for _ in 0..200 {
            assert!(seen.insert(generate_dek()), "DEK collision");
        }
    }

    // ── wrap / unwrap ────────────────────────────────────────────────

    #[test]
    fn wrap_unwrap_roundtrip() {
        let cipher = TokenCipher::from_passphrase("test-system-kek");
        let dek = generate_dek();
        let wrapped = wrap_dek(&cipher, &dek).unwrap();
        assert_eq!(unwrap_dek(&cipher, &wrapped).unwrap(), dek);
    }

    #[test]
    fn wrong_kek_unwrap_fails() {
        let kek1 = TokenCipher::from_passphrase("kek-1");
        let kek2 = TokenCipher::from_passphrase("kek-2");
        let dek = generate_dek();
        let wrapped = wrap_dek(&kek1, &dek).unwrap();
        assert!(unwrap_dek(&kek2, &wrapped).is_err());
    }

    #[test]
    fn wrap_produces_different_ciphertexts_each_call() {
        let cipher = TokenCipher::from_passphrase("kek");
        let dek = generate_dek();
        let w1 = wrap_dek(&cipher, &dek).unwrap();
        let w2 = wrap_dek(&cipher, &dek).unwrap();
        assert_ne!(w1, w2, "each wrap should use a fresh nonce");
    }

    // ── User KEK derivation ──────────────────────────────────────────
    // Marked #[ignore] because Argon2id with 64 MiB is intentionally
    // slow (~200 ms per call).  Run explicitly with:
    //   cargo test vault_crypto::tests::derive -- --ignored --nocapture

    #[test]
    #[ignore = "slow: Argon2id 64 MiB (~200 ms)"]
    fn derive_user_kek_deterministic() {
        let salt = b"0123456789abcdef01234567890abcde"; // 32 bytes
        let k1 = derive_user_kek("my-password", salt).unwrap();
        let k2 = derive_user_kek("my-password", salt).unwrap();
        assert_eq!(k1, k2, "same (password, salt) must yield identical KEK");
    }

    #[test]
    #[ignore = "slow: Argon2id 64 MiB (~200 ms)"]
    fn derive_user_kek_different_passwords() {
        let salt = b"0123456789abcdef01234567890abcde";
        let ka = derive_user_kek("password-A", salt).unwrap();
        let kb = derive_user_kek("password-B", salt).unwrap();
        assert_ne!(ka, kb);
    }

    #[test]
    #[ignore = "slow: Argon2id 64 MiB (~200 ms)"]
    fn derive_user_kek_different_salts() {
        let s1 = b"salt-one-padding-aaaaaaaaaaaaaaaa";
        let s2 = b"salt-two-padding-aaaaaaaaaaaaaaaa";
        let k1 = derive_user_kek("same-password", s1).unwrap();
        let k2 = derive_user_kek("same-password", s2).unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    #[ignore = "slow: Argon2id 64 MiB (~200 ms)"]
    fn kek_domain_separation_from_password_hash() {
        // Two calls with same raw input but different domain prefixes must differ.
        // The domain is hard-coded inside derive_user_kek; verify indirectly by
        // checking the KEK doesn't equal a bare Argon2 hash of the password.
        let salt = b"0123456789abcdef01234567890abcde";
        let kek = derive_user_kek("pw", salt).unwrap();

        // Bare Argon2id with NO domain prefix (simulating auth.rs path).
        let params = Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(32)).unwrap();
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut bare = [0u8; 32];
        argon2.hash_password_into(b"pw", salt, &mut bare).unwrap();

        assert_ne!(kek, bare, "KEK domain must separate from bare hash");
    }
}
