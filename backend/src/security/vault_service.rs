//! High-level vault service: crypto primitives + DB persistence + audit log.
//!
//! Every encrypt/decrypt/delete touching vault data must go through
//! `VaultService`.  Never call `vault_crypto` directly from API handlers.
//!
//! ## Constructing a VaultService
//!
//! ### Phase 1 / System-KEK-only (no User KEK wired yet):
//! ```rust,ignore
//! let vsvc = VaultService::system_only(
//!     &state.db,
//!     state.cipher.clone(),
//!     Some(auth_user.id),
//!     Some(client_ip),
//! );
//! ```
//!
//! ### Phase 2+ / User KEK + System recovery (normal API path):
//! ```rust,ignore
//! let user_kek = state.session_keys
//!     .get_cipher(auth_user.id)
//!     .ok_or(AppError::Unauthorized("session expired, please log in again".into()))?;
//!
//! let vsvc = VaultService::for_user(
//!     &state.db,
//!     std::sync::Arc::new(user_kek),
//!     auth_user.id,
//!     state.cipher.clone(),
//!     auth_user.id,
//!     Some(client_ip),
//! );
//! ```

use anyhow::{anyhow, Result};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    crypto::TokenCipher,
    security::vault_crypto::{generate_dek, open, seal, unwrap_dek, wrap_dek},
};

// ── Service struct ────────────────────────────────────────────────────

pub struct VaultService<'a> {
    db: &'a PgPool,
    /// Primary KEK used for normal encrypt/decrypt (User KEK in Phase 2+,
    /// System KEK in Phase 1 or admin recovery CLI).
    primary_cipher: Arc<TokenCipher>,
    /// `"user:<uuid>"` in Phase 2+, `"system_v1"` in Phase 1.
    primary_alias: String,
    /// Recovery KEK: System KEK.  Present in Phase 2+ so that every sealed
    /// object also gets a System-KEK wrapping for admin recovery.
    recovery_cipher: Option<Arc<TokenCipher>>,
    /// `"system_v1"` when recovery is active, `None` in Phase 1.
    recovery_alias: Option<String>,
    actor_id: Option<Uuid>,
    ip_addr: Option<String>,
}

impl<'a> VaultService<'a> {
    // ── Constructors ──────────────────────────────────────────────────

    /// Phase 2+: User KEK as primary + System KEK recovery wrapping.
    /// Use this in all normal API handlers once auth.rs is wired (Phase 2).
    pub fn for_user(
        db: &'a PgPool,
        user_kek: Arc<TokenCipher>,
        user_id: Uuid,
        system_kek: Arc<TokenCipher>,
        actor_id: Uuid,
        ip_addr: Option<String>,
    ) -> Self {
        Self {
            db,
            primary_cipher: user_kek,
            primary_alias: format!("user:{user_id}"),
            recovery_cipher: Some(system_kek),
            recovery_alias: Some("system_v1".into()),
            actor_id: Some(actor_id),
            ip_addr,
        }
    }

    /// Phase 1 / admin CLI: System KEK only, no User KEK.
    /// Use this before Phase 2 is deployed, or in the `vault_recovery` binary.
    pub fn system_only(
        db: &'a PgPool,
        system_kek: Arc<TokenCipher>,
        actor_id: Option<Uuid>,
        ip_addr: Option<String>,
    ) -> Self {
        Self {
            db,
            primary_cipher: system_kek,
            primary_alias: "system_v1".into(),
            recovery_cipher: None,
            recovery_alias: None,
            actor_id,
            ip_addr,
        }
    }

    // ── Public API ────────────────────────────────────────────────────

    /// Encrypt `plaintext` and persist ciphertext + wrapped DEK(s) to the
    /// database inside a single transaction.
    ///
    /// - In Phase 2+: creates two wrapping rows (User KEK + System KEK).
    /// - In Phase 1:  creates one wrapping row (System KEK only).
    ///
    /// Calling `seal` twice for the same `(object_type, object_id)` is safe:
    /// the ciphertext and wrapping rows are upserted (ON CONFLICT DO UPDATE).
    pub async fn seal(
        &self,
        object_type: &str,
        object_id: Uuid,
        plaintext: &[u8],
    ) -> Result<()> {
        let aad = format!("{object_type}:{object_id}");
        let dek = generate_dek();
        let (nonce, ciphertext) = seal(&dek, plaintext, aad.as_bytes())?;

        let primary_wrapped = wrap_dek(&self.primary_cipher, &dek)?;
        let recovery_wrapped: Option<String> = self
            .recovery_cipher
            .as_deref()
            .map(|rc| wrap_dek(rc, &dek))
            .transpose()?;

        let mut tx = self.db.begin().await?;

        // ── Ciphertext (upsert) ───────────────────────────────────────
        sqlx::query(
            "INSERT INTO vault_ciphertexts
               (object_type, object_id, nonce, ciphertext, aad, plain_size)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (object_id) DO UPDATE
               SET nonce      = EXCLUDED.nonce,
                   ciphertext = EXCLUDED.ciphertext,
                   aad        = EXCLUDED.aad,
                   plain_size = EXCLUDED.plain_size,
                   updated_at = NOW()",
        )
        .bind(object_type)
        .bind(object_id)
        .bind(&nonce[..])
        .bind(&ciphertext)
        .bind(&aad)
        .bind(plaintext.len() as i64)
        .execute(&mut *tx)
        .await?;

        // ── Primary KEK wrapping ──────────────────────────────────────
        upsert_wrapping(&mut *tx, object_type, object_id, &self.primary_alias, &primary_wrapped)
            .await?;

        // ── Recovery KEK wrapping (System KEK, Phase 2+) ─────────────
        if let (Some(alias), Some(wrapped)) = (&self.recovery_alias, &recovery_wrapped) {
            upsert_wrapping(&mut *tx, object_type, object_id, alias, wrapped).await?;
        }

        tx.commit().await?;

        // Audit after commit (fire-and-forget: failure must not roll back the seal).
        let _ = self
            .audit(object_type, object_id, "encrypt", "user", "write_path")
            .await;

        Ok(())
    }

    /// Decrypt an object.
    ///
    /// Tries the primary KEK wrapping first (User KEK path).  If not found
    /// and a recovery cipher is configured, falls back to the System KEK
    /// wrapping and logs a `system_recovery` audit event (high-visibility).
    ///
    /// `reason` is stored in `vault_audit_log.reason` — use a short
    /// constant like `"normal_read"`, `"reveal"`, `"grounding"`, etc.
    pub async fn open(
        &self,
        object_type: &str,
        object_id: Uuid,
        reason: &str,
    ) -> Result<Vec<u8>> {
        let (dek, kek_path) = self.resolve_dek(object_type, object_id).await?;

        // ── Fetch ciphertext row ──────────────────────────────────────
        let row = sqlx::query(
            "SELECT nonce, ciphertext, aad
               FROM vault_ciphertexts
              WHERE object_type = $1 AND object_id = $2",
        )
        .bind(object_type)
        .bind(object_id)
        .fetch_one(self.db)
        .await
        .map_err(|_| {
            anyhow!("vault: ciphertext not found for {object_type}/{object_id}")
        })?;

        let nonce_bytes: Vec<u8> = row.get("nonce");
        let ct: Vec<u8> = row.try_get("ciphertext").map_err(|_| {
            anyhow!("vault: inline ciphertext is NULL (large-object path not yet implemented)")
        })?;
        let aad: String = row.get("aad");

        let nonce: [u8; 12] = nonce_bytes
            .try_into()
            .map_err(|_| anyhow!("vault: corrupt nonce (expected 12 bytes)"))?;

        let plaintext = open(&dek, &nonce, &ct, aad.as_bytes())?;

        // Audit (fire-and-forget).
        let _ = self.audit(object_type, object_id, "decrypt", &kek_path, reason).await;

        Ok(plaintext)
    }

    /// Delete all vault material (ciphertext + all wrappings) for an object.
    /// Call this when the protected object itself is being deleted.
    pub async fn purge(&self, object_type: &str, object_id: Uuid) -> Result<()> {
        let mut tx = self.db.begin().await?;

        sqlx::query(
            "DELETE FROM vault_key_wrappings
              WHERE object_type = $1 AND object_id = $2",
        )
        .bind(object_type)
        .bind(object_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "DELETE FROM vault_ciphertexts
              WHERE object_type = $1 AND object_id = $2",
        )
        .bind(object_type)
        .bind(object_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        let _ = self.audit(object_type, object_id, "delete", "user", "purge").await;

        Ok(())
    }

    /// Return `true` if a ciphertext row exists for this object.
    /// Useful to gate decrypt attempts and avoid misleading errors.
    pub async fn exists(&self, object_type: &str, object_id: Uuid) -> bool {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
               SELECT 1 FROM vault_ciphertexts
                WHERE object_type = $1 AND object_id = $2
             )",
        )
        .bind(object_type)
        .bind(object_id)
        .fetch_one(self.db)
        .await
        .unwrap_or(false)
    }

    // ── Private helpers ───────────────────────────────────────────────

    /// Resolve the DEK for `(object_type, object_id)`.
    /// Returns `(dek, kek_path)` where `kek_path` is `"user"` or
    /// `"system_recovery"` for audit purposes.
    async fn resolve_dek(
        &self,
        object_type: &str,
        object_id: Uuid,
    ) -> Result<([u8; 32], String)> {
        // 1. Try primary KEK wrapping (User KEK in Phase 2+).
        let primary: Option<String> = sqlx::query_scalar(
            "SELECT wrapped_dek FROM vault_key_wrappings
              WHERE object_type = $1 AND object_id = $2 AND kek_alias = $3",
        )
        .bind(object_type)
        .bind(object_id)
        .bind(&self.primary_alias)
        .fetch_optional(self.db)
        .await?;

        if let Some(wrapped) = primary {
            return Ok((unwrap_dek(&self.primary_cipher, &wrapped)?, "user".into()));
        }

        // 2. Fall back to recovery KEK (System KEK), if configured.
        if let (Some(rc), Some(ra)) = (&self.recovery_cipher, &self.recovery_alias) {
            let recovery: Option<String> = sqlx::query_scalar(
                "SELECT wrapped_dek FROM vault_key_wrappings
                  WHERE object_type = $1 AND object_id = $2 AND kek_alias = $3",
            )
            .bind(object_type)
            .bind(object_id)
            .bind(ra)
            .fetch_optional(self.db)
            .await?;

            if let Some(wrapped) = recovery {
                tracing::warn!(
                    actor_id = ?self.actor_id,
                    object_type,
                    %object_id,
                    "VAULT: system_recovery path used — admin System KEK fallback"
                );
                return Ok((unwrap_dek(rc, &wrapped)?, "system_recovery".into()));
            }
        }

        Err(anyhow!(
            "vault: no accessible wrapping for {object_type}/{object_id} \
             (primary_alias={}, recovery={})",
            self.primary_alias,
            self.recovery_alias.as_deref().unwrap_or("none"),
        ))
    }

    async fn audit(
        &self,
        object_type: &str,
        object_id: Uuid,
        operation: &str,
        kek_path: &str,
        reason: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO vault_audit_log
               (actor_id, object_type, object_id, operation, kek_path, reason, ip_addr)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(self.actor_id)
        .bind(object_type)
        .bind(object_id)
        .bind(operation)
        .bind(kek_path)
        .bind(reason)
        .bind(self.ip_addr.as_deref())
        .execute(self.db)
        .await?;
        Ok(())
    }
}

// ── Free helpers ──────────────────────────────────────────────────────

async fn upsert_wrapping(
    conn: &mut sqlx::PgConnection,
    object_type: &str,
    object_id: Uuid,
    kek_alias: &str,
    wrapped_dek: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO vault_key_wrappings
           (object_type, object_id, kek_alias, wrapped_dek)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (object_type, object_id, kek_alias) DO UPDATE
           SET wrapped_dek = EXCLUDED.wrapped_dek",
    )
    .bind(object_type)
    .bind(object_id)
    .bind(kek_alias)
    .bind(wrapped_dek)
    .execute(conn)
    .await?;
    Ok(())
}
