use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

/// AES-256-GCM cipher for encrypting Git access tokens at rest.
///
/// Storage format: base64( nonce(12) || ciphertext_with_tag )
/// Key: 32 bytes, supplied via GIT_TOKEN_ENCRYPTION_KEY env var (base64-encoded)
/// or derived deterministically from a passphrase via SHA-256.
#[derive(Clone)]
pub struct TokenCipher {
    cipher: Aes256Gcm,
}

impl TokenCipher {
    pub fn from_base64_key(key_b64: &str) -> Result<Self> {
        let bytes = B64
            .decode(key_b64.trim())
            .map_err(|e| anyhow!("GIT_TOKEN_ENCRYPTION_KEY is not valid base64: {}", e))?;
        if bytes.len() != 32 {
            return Err(anyhow!(
                "GIT_TOKEN_ENCRYPTION_KEY must decode to exactly 32 bytes, got {}",
                bytes.len()
            ));
        }
        let key = Key::<Aes256Gcm>::from_slice(&bytes);
        Ok(Self {
            cipher: Aes256Gcm::new(key),
        })
    }

    /// Derive a 32-byte key from an arbitrary passphrase via SHA-256.
    /// Used as a dev fallback when GIT_TOKEN_ENCRYPTION_KEY is not set.
    /// Create a cipher directly from a 32-byte raw key (e.g. a derived User KEK).
    /// The key bytes are copied into the cipher's internal state; the caller
    /// can safely zeroize their copy afterwards.
    pub fn from_raw_key(key: &[u8; 32]) -> Self {
        let k = Key::<Aes256Gcm>::from_slice(key);
        Self {
            cipher: Aes256Gcm::new(k),
        }
    }

    pub fn from_passphrase(passphrase: &str) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"kway-dev-token-cipher-v1::");
        hasher.update(passphrase.as_bytes());
        let digest = hasher.finalize();
        let key = Key::<Aes256Gcm>::from_slice(&digest);
        Self {
            cipher: Aes256Gcm::new(key),
        }
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|e| anyhow!("encrypt failed: {}", e))?;

        let mut combined = Vec::with_capacity(nonce.len() + ciphertext.len());
        combined.extend_from_slice(&nonce);
        combined.extend_from_slice(&ciphertext);
        Ok(B64.encode(&combined))
    }

    pub fn decrypt(&self, ciphertext_b64: &str) -> Result<String> {
        let combined = B64
            .decode(ciphertext_b64)
            .map_err(|e| anyhow!("decrypt: invalid base64: {}", e))?;
        if combined.len() < 12 + 16 {
            return Err(anyhow!("decrypt: payload too short"));
        }
        let (nonce_bytes, body) = combined.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        let plaintext = self
            .cipher
            .decrypt(nonce, body)
            .map_err(|e| anyhow!("decrypt failed (key mismatch or corrupt data): {}", e))?;
        String::from_utf8(plaintext).map_err(|e| anyhow!("decrypt: invalid utf-8: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let cipher = TokenCipher::from_passphrase("test-passphrase");
        let encrypted = cipher.encrypt("ghp_secret_token_12345").unwrap();
        let decrypted = cipher.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "ghp_secret_token_12345");
    }

    #[test]
    fn different_nonces_each_call() {
        let cipher = TokenCipher::from_passphrase("test");
        let a = cipher.encrypt("same-input").unwrap();
        let b = cipher.encrypt("same-input").unwrap();
        assert_ne!(a, b, "each encryption should produce different ciphertext");
    }

    #[test]
    fn wrong_key_fails() {
        let c1 = TokenCipher::from_passphrase("key-1");
        let c2 = TokenCipher::from_passphrase("key-2");
        let encrypted = c1.encrypt("secret").unwrap();
        assert!(c2.decrypt(&encrypted).is_err());
    }
}
