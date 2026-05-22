//! Vault admin CLI — offline emergency recovery and System KEK rotation.
//!
//! This tool operates with the **System KEK** (`GIT_TOKEN_ENCRYPTION_KEY`).
//! Every plaintext-access operation is audit-logged as `kek_path='system_recovery'`
//! so it appears in the security audit trail alongside normal user decrypts.
//!
//! ─────────────────────────────────────────────────────────────────────────────
//!  SUBCOMMANDS
//! ─────────────────────────────────────────────────────────────────────────────
//!
//!  list
//!    Show every sealed object (type, UUID, wrapping aliases, plaintext size).
//!    No decryption performed — safe to run at any time.
//!
//!  audit [--limit N]
//!    Tail the vault_audit_log.  Default: last 50 entries.
//!
//!  recover --type <OBJECT_TYPE> --id <UUID>
//!    Emergency-decrypt one object using the System KEK and print the plaintext
//!    to stdout.  Use only when the owner cannot log in themselves.
//!    ⚠  Output may contain raw secrets — pipe carefully.
//!
//!  rotate-system-key
//!    Generate a new 32-byte System KEK.
//!    1. Registers it as `system_v2` (or next available alias) in vault_keys.
//!    2. Re-wraps every `system_v1`-keyed DEK with the new key.
//!    3. Marks `system_v1` as 'retired' in vault_keys.
//!    4. Prints the new key value for the operator to save in `.env`.
//!    The old wrappings are left intact (safety net until confirmed stable).
//!
//! ─────────────────────────────────────────────────────────────────────────────
//!  USAGE
//! ─────────────────────────────────────────────────────────────────────────────
//!
//!  cargo run --release --bin vault_admin -- list
//!  cargo run --release --bin vault_admin -- audit --limit 100
//!  cargo run --release --bin vault_admin -- recover --type vault_secret --id <UUID>
//!  cargo run --release --bin vault_admin -- rotate-system-key
//!
//! Required env vars: DATABASE_URL, GIT_TOKEN_ENCRYPTION_KEY
//! (load via .env automatically — `dotenvy::dotenv().ok()` is called at startup)

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    AeadCore, Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use sqlx::{PgPool, Row};
use uuid::Uuid;

// ── Inline crypto primitives ──────────────────────────────────────────────────
// Mirrors security/vault_crypto.rs and crypto.rs.  Duplicated here so this
// binary has zero dependency on main.rs internals — the CLI must compile and
// run even if the server module tree changes.

struct SystemKek {
    cipher: Aes256Gcm,
}

impl SystemKek {
    fn from_base64(key_b64: &str) -> Result<Self> {
        let bytes = B64
            .decode(key_b64.trim())
            .map_err(|e| anyhow!("GIT_TOKEN_ENCRYPTION_KEY: invalid base64: {e}"))?;
        if bytes.len() != 32 {
            bail!(
                "GIT_TOKEN_ENCRYPTION_KEY must decode to 32 bytes, got {}",
                bytes.len()
            );
        }
        Ok(Self {
            cipher: Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&bytes)),
        })
    }

    /// Dev fallback when the env var is absent (uses SHA-256 of a passphrase).
    fn from_passphrase(passphrase: &str) -> Self {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"kway-dev-token-cipher-v1::");
        h.update(passphrase.as_bytes());
        let digest = h.finalize();
        Self {
            cipher: Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&digest)),
        }
    }

    /// `wrapped_dek` = base64( nonce || AES-GCM(base64(dek_bytes)) )
    fn unwrap_dek(&self, wrapped: &str) -> Result<[u8; 32]> {
        let combined = B64
            .decode(wrapped)
            .map_err(|e| anyhow!("unwrap_dek: base64: {e}"))?;
        if combined.len() < 28 {
            bail!("unwrap_dek: payload too short ({} bytes)", combined.len());
        }
        let (nonce_bytes, body) = combined.split_at(12);
        let plain = self
            .cipher
            .decrypt(Nonce::from_slice(nonce_bytes), body)
            .map_err(|e| anyhow!("unwrap_dek: auth tag mismatch — wrong key? {e}"))?;

        // The inner plaintext is the DEK base64-encoded.
        let dek_b64 = String::from_utf8(plain)
            .map_err(|e| anyhow!("unwrap_dek: utf-8: {e}"))?;
        let dek_bytes = B64
            .decode(&dek_b64)
            .map_err(|e| anyhow!("unwrap_dek: inner base64: {e}"))?;
        dek_bytes
            .try_into()
            .map_err(|_| anyhow!("unwrap_dek: expected 32 bytes after decode"))
    }

    fn wrap_dek(&self, dek: &[u8; 32]) -> Result<String> {
        let dek_b64 = B64.encode(dek);
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ct = self
            .cipher
            .encrypt(&nonce, dek_b64.as_bytes())
            .map_err(|e| anyhow!("wrap_dek: {e}"))?;
        let mut combined = Vec::with_capacity(12 + ct.len());
        combined.extend_from_slice(&nonce);
        combined.extend_from_slice(&ct);
        Ok(B64.encode(&combined))
    }

    // ── Git-token helpers (TokenCipher format: nonce||ciphertext, no inner b64) ──

    /// Decrypt a git access token stored in `git_identities.access_token`.
    /// Format: base64( nonce(12) || AES-GCM(token_bytes) )
    /// (Same as crypto::TokenCipher — no inner base64 layer.)
    fn decrypt_token(&self, encrypted: &str) -> Result<String> {
        let combined = B64
            .decode(encrypted.trim())
            .map_err(|e| anyhow!("decrypt_token: base64: {e}"))?;
        if combined.len() < 13 {
            bail!("decrypt_token: payload too short ({} bytes)", combined.len());
        }
        let (nonce_bytes, body) = combined.split_at(12);
        let plain = self
            .cipher
            .decrypt(Nonce::from_slice(nonce_bytes), body)
            .map_err(|e| anyhow!("decrypt_token: auth tag mismatch — wrong key? {e}"))?;
        String::from_utf8(plain).map_err(|e| anyhow!("decrypt_token: utf-8: {e}"))
    }

    /// Encrypt a git access token with this KEK.
    fn encrypt_token(&self, token: &str) -> Result<String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ct = self
            .cipher
            .encrypt(&nonce, token.as_bytes())
            .map_err(|e| anyhow!("encrypt_token: {e}"))?;
        let mut combined = Vec::with_capacity(12 + ct.len());
        combined.extend_from_slice(&nonce);
        combined.extend_from_slice(&ct);
        Ok(B64.encode(&combined))
    }
}

/// AES-256-GCM decrypt using a raw DEK (for vault_ciphertexts rows).
fn aes_open(dek: &[u8; 32], nonce: &[u8; 12], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(dek));
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            aes_gcm::aead::Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|e| anyhow!("aes_open (auth tag mismatch — corrupt data or wrong key): {e}"))
}

// ── Environment setup ─────────────────────────────────────────────────────────

fn load_system_kek() -> Result<SystemKek> {
    match std::env::var("GIT_TOKEN_ENCRYPTION_KEY").ok() {
        Some(k) if !k.trim().is_empty() => SystemKek::from_base64(&k),
        _ => {
            eprintln!(
                "⚠  GIT_TOKEN_ENCRYPTION_KEY is not set — \
                 using insecure dev passphrase fallback"
            );
            Ok(SystemKek::from_passphrase("kway-dev-fallback"))
        }
    }
}

async fn connect_db() -> Result<PgPool> {
    let url =
        std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    PgPool::connect(&url)
        .await
        .context("failed to connect to PostgreSQL")
}

// ── Subcommands ───────────────────────────────────────────────────────────────

/// `vault_admin list`
///
/// Print a table of every sealed object and the KEK aliases covering it.
async fn cmd_list(pool: &PgPool) -> Result<()> {
    let rows = sqlx::query(
        "SELECT kw.object_type, kw.object_id, kw.kek_alias, kw.wrap_alg,
                vc.plain_size
           FROM vault_key_wrappings kw
           LEFT JOIN vault_ciphertexts vc
             ON vc.object_type = kw.object_type AND vc.object_id = kw.object_id
          ORDER BY kw.object_type, kw.object_id, kw.kek_alias",
    )
    .fetch_all(pool)
    .await
    .context("failed to query vault_key_wrappings")?;

    if rows.is_empty() {
        println!("No vault objects found.");
        return Ok(());
    }

    println!(
        "\n{:<18} {:<38}  {:<34} {:<16} {}",
        "OBJECT_TYPE", "OBJECT_ID", "KEK_ALIAS", "ALGORITHM", "PLAIN_BYTES"
    );
    println!("{}", "─".repeat(115));

    let mut unique_objects = std::collections::HashSet::new();
    for row in &rows {
        let otype: String = row.get("object_type");
        let oid: Uuid = row.get("object_id");
        let alias: String = row.get("kek_alias");
        let alg: String = row.get("wrap_alg");
        let size: Option<i64> = row.get("plain_size");
        let size_str = size
            .map(|n| n.to_string())
            .unwrap_or_else(|| "?".into());
        println!(
            "{:<18} {:<38}  {:<34} {:<16} {}",
            otype, oid, alias, alg, size_str
        );
        unique_objects.insert(oid);
    }

    println!(
        "\n{} wrapping row(s) across {} sealed object(s).",
        rows.len(),
        unique_objects.len()
    );

    // Warn about any objects that have a user-KEK wrapping but NO system alias
    // wrapping at all (regardless of rotation generation).  An object is only
    // unrecoverable if it has zero non-user rows — having system_v2 instead of
    // system_v1 is fine after a key rotation.
    let missing_system: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT object_id FROM vault_key_wrappings
          WHERE kek_alias LIKE 'user:%'
            AND object_id NOT IN (
              SELECT object_id FROM vault_key_wrappings
               WHERE kek_alias NOT LIKE 'user:%'
            )",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    if !missing_system.is_empty() {
        println!(
            "\n⚠  {} object(s) have a user-KEK wrapping but NO system-alias \
             recovery wrapping — admin cannot recover these:\n",
            missing_system.len()
        );
        for id in &missing_system {
            println!("   {id}");
        }
    }

    Ok(())
}

/// `vault_admin audit [--limit N]`
///
/// Tail the vault_audit_log.  Default limit: 50 entries.
async fn cmd_audit(pool: &PgPool, limit: i64) -> Result<()> {
    let rows = sqlx::query(
        "SELECT al.id, al.actor_id, al.object_type, al.object_id,
                al.operation, al.kek_path, al.reason, al.ip_addr, al.created_at
           FROM vault_audit_log al
          ORDER BY al.created_at DESC
          LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .context("failed to query vault_audit_log")?;

    if rows.is_empty() {
        println!("No audit log entries found.");
        return Ok(());
    }

    println!(
        "\n{:<26} {:<12} {:<18} {:<12} {:<18} {}",
        "TIMESTAMP", "OPERATION", "KEK_PATH", "OBJECT_TYPE", "OBJECT_ID", "REASON"
    );
    println!("{}", "─".repeat(110));

    for row in &rows {
        let created_at: chrono::DateTime<chrono::Utc> = row.get("created_at");
        let operation: String = row.get("operation");
        let kek_path: String = row.get("kek_path");
        let otype: String = row.get("object_type");
        let oid: Uuid = row.get("object_id");
        let reason: String = row.get("reason");
        println!(
            "{:<26} {:<12} {:<18} {:<12} {:<38} {}",
            created_at.format("%Y-%m-%d %H:%M:%S UTC"),
            operation,
            kek_path,
            otype,
            oid,
            reason
        );
    }

    println!("\n(showing last {limit} entries)");
    Ok(())
}

/// `vault_admin recover --type <OBJECT_TYPE> --id <UUID>`
///
/// Emergency-decrypt one vault object using the System KEK.
/// Outputs the plaintext bytes as UTF-8 to stdout.
/// Writes a `system_recovery` audit log entry.
///
/// After a `rotate-system-key` the active alias changes (e.g. system_v1 →
/// system_v2).  This command fetches ALL non-user-alias wrappings for the
/// object and tries them newest-first, so it works regardless of whether
/// `.env` has already been updated with the post-rotation key or not.
async fn cmd_recover(pool: &PgPool, object_type: &str, object_id: Uuid) -> Result<()> {
    let kek = load_system_kek()?;

    // 1. Fetch ALL system-alias-wrapped DEKs for this object (non-user aliases),
    //    ordered newest first so we try the most recently rotated wrapping first.
    //    This handles both the "just rotated, .env already updated" case (system_v2
    //    row is tried and succeeds) and the "rotating in progress, .env not yet
    //    updated" case (system_v2 fails with wrong key → system_v1 succeeds).
    let alias_rows = sqlx::query(
        "SELECT kek_alias, wrapped_dek FROM vault_key_wrappings
          WHERE object_type = $1 AND object_id = $2
            AND kek_alias NOT LIKE 'user:%'
          ORDER BY created_at DESC",
    )
    .bind(object_type)
    .bind(object_id)
    .fetch_all(pool)
    .await
    .context("failed to query vault_key_wrappings")?;

    if alias_rows.is_empty() {
        bail!(
            "No system-alias wrapping found for {object_type}/{object_id}. \
             This object may have been created before system recovery was enabled."
        );
    }

    // Try each alias in reverse-creation order (newest first).
    let mut tried: Vec<String> = Vec::new();
    let mut dek_result: Option<([u8; 32], String)> = None;

    for row in &alias_rows {
        let alias: String = row.get("kek_alias");
        let wrapped: String = row.get("wrapped_dek");
        tried.push(alias.clone());
        match kek.unwrap_dek(&wrapped) {
            Ok(dek) => {
                dek_result = Some((dek, alias));
                break;
            }
            Err(e) => {
                eprintln!("  alias '{alias}': {e} — trying next …");
            }
        }
    }

    let (dek, used_alias) = dek_result.ok_or_else(|| {
        anyhow!(
            "Could not unwrap DEK for {object_type}/{object_id} with the current System KEK.\n\
             Tried aliases (newest first): {}\n\n\
             Possible causes:\n\
             • GIT_TOKEN_ENCRYPTION_KEY does not match the key used when these rows were written.\n\
             • If you just ran rotate-system-key, update .env with the new key and retry.\n\
             • If the key was never set, the dev-passphrase fallback was used — ensure the same \
               fallback is active now.",
            tried.join(", ")
        )
    })?;

    // 2. Fetch ciphertext row.
    let row = sqlx::query(
        "SELECT nonce, ciphertext, aad
           FROM vault_ciphertexts
          WHERE object_type = $1 AND object_id = $2",
    )
    .bind(object_type)
    .bind(object_id)
    .fetch_optional(pool)
    .await
    .context("failed to query vault_ciphertexts")?
    .ok_or_else(|| {
        anyhow!("No ciphertext found for {object_type}/{object_id}")
    })?;

    let nonce_bytes: Vec<u8> = row.get("nonce");
    let ciphertext: Vec<u8> = row.try_get("ciphertext")
        .map_err(|_| anyhow!("ciphertext is NULL (large-object path not implemented)"))?;
    let aad: String = row.get("aad");

    let nonce: [u8; 12] = nonce_bytes
        .try_into()
        .map_err(|_| anyhow!("corrupt nonce: expected 12 bytes"))?;

    let plaintext = aes_open(&dek, &nonce, &ciphertext, aad.as_bytes())?;

    // 3. Write audit log entry (system_recovery path, high-visibility).
    let _ = sqlx::query(
        "INSERT INTO vault_audit_log
           (object_type, object_id, operation, kek_path, reason)
         VALUES ($1, $2, 'decrypt', 'system_recovery', 'admin_cli_recover')",
    )
    .bind(object_type)
    .bind(object_id)
    .execute(pool)
    .await;

    // 4. Output plaintext.
    //    For text values: print to stdout.
    //    For binary values: print as hex with a warning.
    match std::str::from_utf8(&plaintext) {
        Ok(text) => {
            eprintln!(
                "✓  Decrypted {object_type}/{object_id} via alias '{used_alias}' (audit logged)"
            );
            eprintln!("───────────────────────────────────────────────────────────");
            println!("{text}");
        }
        Err(_) => {
            eprintln!(
                "⚠  Plaintext is not valid UTF-8 (alias '{used_alias}') — printing as hex:"
            );
            println!(
                "{}",
                plaintext
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }

    Ok(())
}

/// `vault_admin rotate-system-key`
///
/// 1. Generate a new random 32-byte System KEK.
/// 2. Register it as the next alias in vault_keys (system_v2, system_v3, ...).
/// 3. Re-wrap every DEK currently wrapped with the active system alias.
/// 4. Retire the old alias in vault_keys.
/// 5. Print the new key value so the operator can update .env.
///
/// The old system alias wrappings are left in vault_key_wrappings as a
/// safety net until the operator confirms the new key works.
async fn cmd_rotate_system_key(pool: &PgPool) -> Result<()> {
    let old_kek = load_system_kek()?;

    // Determine the current active system alias.
    let active_alias: Option<String> = sqlx::query_scalar(
        "SELECT alias FROM vault_keys
          WHERE status = 'active'
          ORDER BY created_at DESC
          LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .context("failed to query vault_keys")?;

    let old_alias = active_alias.unwrap_or_else(|| "system_v1".into());

    // Derive the next alias name (system_v1 → system_v2 → system_v3 ...).
    let new_alias = if let Some(suffix) = old_alias.strip_prefix("system_v") {
        let n: u64 = suffix.parse().unwrap_or(1);
        format!("system_v{}", n + 1)
    } else {
        format!("{old_alias}_next")
    };

    println!("Current active alias : {old_alias}");
    println!("New alias            : {new_alias}");

    // Generate new 32-byte KEK.
    let mut new_key_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut new_key_bytes);
    let new_key_b64 = B64.encode(new_key_bytes);
    let new_kek = SystemKek::from_base64(&new_key_b64)?;

    // Register new alias in vault_keys.
    sqlx::query(
        "INSERT INTO vault_keys (alias) VALUES ($1)
         ON CONFLICT (alias) DO NOTHING",
    )
    .bind(&new_alias)
    .execute(pool)
    .await
    .context("failed to register new alias in vault_keys")?;

    // Fetch all DEKs wrapped with the old alias.
    let wrappings = sqlx::query(
        "SELECT object_type, object_id, wrapped_dek
           FROM vault_key_wrappings
          WHERE kek_alias = $1",
    )
    .bind(&old_alias)
    .fetch_all(pool)
    .await
    .context("failed to query vault_key_wrappings")?;

    let total = wrappings.len();
    println!("Re-wrapping {total} DEK(s) …");

    let mut rewrapped = 0usize;
    let mut failures = 0usize;

    for row in &wrappings {
        let otype: String = row.get("object_type");
        let oid: Uuid = row.get("object_id");
        let wrapped: String = row.get("wrapped_dek");

        match old_kek.unwrap_dek(&wrapped) {
            Ok(dek) => match new_kek.wrap_dek(&dek) {
                Ok(new_wrapped) => {
                    let res = sqlx::query(
                        "INSERT INTO vault_key_wrappings
                           (object_type, object_id, kek_alias, wrapped_dek)
                         VALUES ($1, $2, $3, $4)
                         ON CONFLICT (object_type, object_id, kek_alias) DO UPDATE
                           SET wrapped_dek = EXCLUDED.wrapped_dek",
                    )
                    .bind(&otype)
                    .bind(oid)
                    .bind(&new_alias)
                    .bind(&new_wrapped)
                    .execute(pool)
                    .await;

                    match res {
                        Ok(_) => {
                            // Audit the re-wrap event.
                            let _ = sqlx::query(
                                "INSERT INTO vault_audit_log
                                   (object_type, object_id, operation, kek_path, reason)
                                 VALUES ($1, $2, 'rewrap', 'system_recovery', $3)",
                            )
                            .bind(&otype)
                            .bind(oid)
                            .bind(format!("rotate:{old_alias}->{new_alias}"))
                            .execute(pool)
                            .await;
                            rewrapped += 1;
                        }
                        Err(e) => {
                            eprintln!("  ✗ DB write failed for {otype}/{oid}: {e}");
                            failures += 1;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("  ✗ wrap_dek failed for {otype}/{oid}: {e}");
                    failures += 1;
                }
            },
            Err(e) => {
                eprintln!("  ✗ unwrap_dek failed for {otype}/{oid}: {e}");
                eprintln!("    (this object cannot be recovered with the current System KEK)");
                failures += 1;
            }
        }
    }

    if failures > 0 {
        eprintln!(
            "\n⚠  {failures}/{total} DEK(s) could not be re-wrapped. \
             NOT retiring the old alias — investigate failures first.\n"
        );
        bail!("rotation incomplete due to {failures} failure(s)");
    }

    // ── Re-encrypt git_identities.access_token ────────────────────────────────
    // These tokens are stored as direct AES-GCM blobs (not via vault envelope),
    // so they must be re-encrypted separately when the System KEK rotates.
    // Tokens that were encrypted with a User KEK (Phase 2+ path) will fail to
    // decrypt here; that's expected — they don't need re-encryption because
    // identity_credentials already has a User-KEK→System-KEK fallback and the
    // User KEK path is unaffected by System KEK rotation.
    println!("Re-encrypting git identity tokens …");
    let git_rows = sqlx::query(
        "SELECT id, access_token FROM git_identities",
    )
    .fetch_all(pool)
    .await
    .context("failed to query git_identities")?;

    let git_total = git_rows.len();
    let mut git_ok = 0usize;
    let mut git_skip = 0usize;

    for row in &git_rows {
        let id: Uuid = row.get("id");
        let encrypted: String = row.get("access_token");

        // Try decrypting with the OLD System KEK.
        match old_kek.decrypt_token(&encrypted) {
            Ok(plaintext_token) => {
                // Re-encrypt with the new System KEK.
                match new_kek.encrypt_token(&plaintext_token) {
                    Ok(new_encrypted) => {
                        match sqlx::query(
                            "UPDATE git_identities SET access_token = $1 WHERE id = $2",
                        )
                        .bind(&new_encrypted)
                        .bind(id)
                        .execute(pool)
                        .await
                        {
                            Ok(_) => git_ok += 1,
                            Err(e) => {
                                eprintln!("  ✗ DB update failed for git_identity {id}: {e}");
                                // Non-fatal: log and continue so other identities still get rotated.
                            }
                        }
                    }
                    Err(e) => eprintln!("  ✗ encrypt_token failed for {id}: {e}"),
                }
            }
            Err(_) => {
                // Decryption failed → token was encrypted with the User KEK (Phase 2+)
                // or a different System KEK.  Skip gracefully.
                git_skip += 1;
            }
        }
    }

    println!(
        "  ✓ {git_ok}/{git_total} git token(s) re-encrypted \
         ({git_skip} skipped — User-KEK-encrypted or unknown key)."
    );

    // All re-wraps succeeded — retire the old alias.
    sqlx::query(
        "UPDATE vault_keys
            SET status = 'retired', retired_at = NOW()
          WHERE alias = $1",
    )
    .bind(&old_alias)
    .execute(pool)
    .await
    .context("failed to retire old alias")?;

    println!("  ✓ {rewrapped}/{total} DEK(s) re-wrapped successfully.");
    println!("  ✓ Alias '{old_alias}' retired.");
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║  NEW GIT_TOKEN_ENCRYPTION_KEY (save this NOW, never log it)  ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  GIT_TOKEN_ENCRYPTION_KEY={new_key_b64}");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!(
        "\nNext steps:\n\
         1. Update GIT_TOKEN_ENCRYPTION_KEY in backend/.env (or your secret manager).\n\
         2. Restart the backend server so new vault sessions use the new KEK.\n\
         3. Verify: `vault_admin list` should show '{new_alias}' wrappings for all objects.\n\
         4. Optional cleanup: once stable, DELETE FROM vault_key_wrappings WHERE kek_alias = '{old_alias}';\n"
    );

    Ok(())
}

// ── Argument parsing ──────────────────────────────────────────────────────────

fn parse_flag<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].as_str())
}

fn usage() -> ! {
    eprintln!(
        r#"
vault_admin — Kway Vault admin CLI

USAGE:
  vault_admin list
  vault_admin audit [--limit N]
  vault_admin recover --type <OBJECT_TYPE> --id <UUID>
  vault_admin rotate-system-key

ENVIRONMENT:
  DATABASE_URL               PostgreSQL connection string (required)
  GIT_TOKEN_ENCRYPTION_KEY   Base64-encoded 32-byte System KEK (required for recover / rotate)
"#
    );
    std::process::exit(1);
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let all_args: Vec<String> = std::env::args().collect();
    let args: &[String] = if all_args.len() > 1 { &all_args[1..] } else { &[] };

    if args.is_empty() {
        usage();
    }

    let pool = connect_db().await?;

    match args[0].as_str() {
        "list" => {
            cmd_list(&pool).await?;
        }
        "audit" => {
            let limit: i64 = parse_flag(args, "--limit")
                .and_then(|v| v.parse().ok())
                .unwrap_or(50);
            cmd_audit(&pool, limit).await?;
        }
        "recover" => {
            let object_type = parse_flag(args, "--type")
                .ok_or_else(|| anyhow!("--type is required for 'recover'"))?;
            let id_str = parse_flag(args, "--id")
                .ok_or_else(|| anyhow!("--id is required for 'recover'"))?;
            let object_id = id_str
                .parse::<Uuid>()
                .map_err(|_| anyhow!("--id must be a valid UUID"))?;
            cmd_recover(&pool, object_type, object_id).await?;
        }
        "rotate-system-key" => {
            println!("⚠  This will retire the current System KEK and generate a new one.");
            println!("   Make sure you have a backup of the current GIT_TOKEN_ENCRYPTION_KEY.");
            println!("   Press Enter to continue, or Ctrl+C to abort.");
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).ok();
            cmd_rotate_system_key(&pool).await?;
        }
        other => {
            eprintln!("Unknown subcommand: {other}");
            usage();
        }
    }

    Ok(())
}
