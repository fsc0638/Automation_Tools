# Vault Recovery Runbook

**System:** Kway Dev Platform — Data Vault (Option B: User KEK + System Recovery)  
**Last updated:** 2026-05-21  
**Audience:** Infrastructure team / on-call admin  

---

## Architecture overview

```
Login:
  Argon2id(user_password, users.kek_salt)  ──►  User KEK  (RAM only, never stored)

Seal an object:
  OsRng ──► random DEK (32 bytes)
  DEK ──AES-256-GCM──► ciphertext         → vault_ciphertexts
  DEK ──wrap(User KEK)──► wrapped_dek     → vault_key_wrappings  kek_alias='user:<uuid>'
  DEK ──wrap(System KEK)──► wrapped_dek   → vault_key_wrappings  kek_alias='system_v1'

Normal decrypt (user browser session):
  wrapped_dek[user:<uuid>] ──unwrap(User KEK)──► DEK ──► plaintext

Admin recovery (this runbook):
  wrapped_dek[system_v1] ──unwrap(System KEK)──► DEK ──► plaintext
  + mandatory audit log entry (kek_path='system_recovery')
```

**Key invariant:** every object sealed by the application gets **two** wrapping rows — one for the owning user (session-gated) and one for the System KEK (admin/recovery). If the System KEK is lost, admin recovery is impossible.

---

## Prerequisites

Before running any command:

```bash
# 1. Set env vars
export DATABASE_URL="postgres://postgres:<password>@localhost:5432/kway_dev"
export GIT_TOKEN_ENCRYPTION_KEY="<base64-encoded 32-byte key from .env>"

# 2. Load .env automatically (if present in backend/):
cd backend/
```

All CLI commands use `dotenvy::dotenv()` so they pick up `backend/.env` automatically.

---

## CLI reference

```bash
# Build once (reuse across incidents):
cargo build --release --bin vault_admin

# Shorthand for commands below:
alias va="./target/release/vault_admin"
```

### `list` — Inspect vault objects

```bash
va list
```

Output:
- Table of all sealed objects (type, UUID, KEK aliases, plaintext size)
- Warning if any object is missing a `system_v1` wrapping (recovery gap)

Use this first in any incident to understand what is sealed and what aliases cover it.

---

### `audit` — View the audit trail

```bash
va audit                  # last 50 entries (default)
va audit --limit 200      # last 200 entries
```

Output columns: timestamp, operation, kek_path, object_type, object_id, reason.

`kek_path = system_recovery` entries indicate admin CLI accesses — each one appears here.

---

### `recover` — Emergency decrypt

```bash
va recover --type vault_secret --id <UUID>
```

**When to use:** A user cannot log in (e.g. forgot password, server restart wiped KEK session) and needs their secret urgently.

**What happens:**
1. Queries **all** non-user-alias wrappings for the object (e.g. `system_v1`, `system_v2`, …), newest first.
2. Tries each wrapping with the current `GIT_TOKEN_ENCRYPTION_KEY` until one decrypts successfully.
3. Decrypts the ciphertext with the recovered DEK.
4. Writes a `system_recovery` audit log entry.
5. Prints plaintext to **stdout** — the caller is responsible for secure handling.

This means `recover` works both **before** and **after** updating `.env` following a `rotate-system-key` — if the new key is in `.env` the newest wrapping wins; if not, the older wrapping is tried automatically.

**⚠ Security notes:**
- Never paste the output into chat/email/tickets.
- Pipe to a file or read directly:
  ```bash
  va recover --type vault_secret --id <UUID> > /tmp/secret.txt
  # transfer securely, then:
  shred -u /tmp/secret.txt
  ```
- The audit log permanently records this access.

**Find the right UUID:**

```sql
-- List secrets for a specific user by email:
SELECT vs.id, vs.label, vs.secret_type, vs.created_at
FROM vault_secrets vs
JOIN users u ON u.id = vs.user_id
WHERE u.email = 'user@example.com';
```

---

### `rotate-system-key` — System KEK rotation

```bash
va rotate-system-key
```

**When to use:**
- Scheduled periodic rotation (recommended: every 90 days)
- Suspected System KEK compromise
- Staff changes that may have exposed the key

**What happens:**
1. Prompts for confirmation.
2. Generates a new 32-byte random key.
3. Registers it as `system_v2` (or next sequential alias) in `vault_keys`.
4. Re-wraps every DEK currently under the active alias with the new key.
5. Marks the old alias `'retired'` in `vault_keys`.
6. Prints the new key value.

**After rotation:**

```bash
# 1. Save the printed key value:
#    GIT_TOKEN_ENCRYPTION_KEY=<new_base64_value>
#    Update backend/.env (or your secret manager / Kubernetes secret).

# 2. Restart the backend (new vault sessions will use the new KEK):
#    docker compose restart backend
#    — or —
#    cargo run --release

# 3. Verify all objects now have the new alias:
va list   # should show system_v2 (or system_vN) alongside user wrappings

# 4. Optional cleanup — remove old system_v1 wrappings once stable (≥ 24h):
#    psql $DATABASE_URL -c "DELETE FROM vault_key_wrappings WHERE kek_alias = 'system_v1';"
```

**⚠ If rotation fails partway:** the CLI aborts without retiring the old alias. The partial `system_v2` rows are present but harmless. Re-run after fixing the underlying issue (bad key, DB connectivity, etc.) — the upsert is idempotent.

---

## Scenario playbook

### S1: User locked out after server restart

User KEKs are RAM-only and lost on restart. The user must log in again — their vault is accessible immediately after login (User KEK is re-derived from password at login time).

**No CLI action needed.** Tell the user to log in.

### S2: User forgot password and needs vault secret urgently

1. Confirm the request is legitimate (identity check out-of-band).
2. `va recover --type vault_secret --id <UUID>`
3. Deliver plaintext via a secure channel (Signal, in-person, encrypted email).
4. Reset the user's password normally (via the app or DB). Their User KEK will change, so their old vault entries need re-wrapping. **The user must re-add their vault secrets after the password reset** — there is no automated re-wrap for the User KEK path without the old password.

### S3: GIT_TOKEN_ENCRYPTION_KEY suspected compromised

1. **Do not delete the old key yet.**
2. `va rotate-system-key` — re-wraps all DEKs under a new key.
3. Update `.env` / secret manager with the new key.
4. Restart backend.
5. Run `va list` to confirm `system_v2` wrappings are present.
6. After 24h stability window, delete old `system_v1` wrapping rows.

### S4: Scheduled 90-day rotation

Same as S3 (non-emergency, no urgency). Use the same steps.

### S5: Database restore / migration

After restoring a DB backup:
- Vault ciphertexts and wrappings are intact (part of the backup).
- The System KEK (`GIT_TOKEN_ENCRYPTION_KEY`) is in `.env`, not the DB — ensure `.env` is consistent with the backup's era.
- Users must log in again to re-establish User KEK sessions.

---

## Monitoring queries

```sql
-- All system_recovery accesses in the last 7 days:
SELECT actor_id, object_type, object_id, reason, ip_addr, created_at
FROM vault_audit_log
WHERE kek_path = 'system_recovery'
  AND created_at > NOW() - INTERVAL '7 days'
ORDER BY created_at DESC;

-- Objects with no system wrapping (recovery gap):
SELECT DISTINCT kw.object_type, kw.object_id
FROM vault_key_wrappings kw
WHERE kw.kek_alias LIKE 'user:%'
  AND kw.object_id NOT IN (
    SELECT object_id FROM vault_key_wrappings
    WHERE kek_alias IN (SELECT alias FROM vault_keys WHERE status = 'active')
  );

-- Current vault_keys status:
SELECT alias, status, created_at, retired_at FROM vault_keys ORDER BY created_at;

-- Vault object count by type:
SELECT object_type, COUNT(DISTINCT object_id) AS objects
FROM vault_key_wrappings
GROUP BY object_type;
```

---

## Key storage recommendations

| Environment | Recommended storage |
|---|---|
| Local dev | `backend/.env` (never committed) |
| Staging | Environment variable in docker-compose or CI secrets |
| Production | Kubernetes Secret / AWS Secrets Manager / HashiCorp Vault |

**Never log the key value.** The `rotate-system-key` command prints it once to stdout — copy it immediately and clear the terminal history.

```bash
# Clear terminal history after handling a key rotation:
history -c   # bash
```

---

## Per-user encrypted disk images (macOS)

When `DMG_ROOT` is set in `.env`, the backend automatically manages a personal
APFS-encrypted sparse image for every user.

### How it works

| Event | Backend action |
|---|---|
| First login | Generates 32-byte random key → seals in vault as `user_dmg_key` → creates `<DMG_ROOT>/<user_id>.sparseimage` → mounts at `<PROJECT_DATA_ROOT>/users/<user_id>/` |
| Subsequent login | Opens key from vault → mounts (idempotent) |
| Logout | `hdiutil detach` the user's mount point |

All project files, meeting attachments, and git clones for a user land inside
their mounted image.  When they are logged out the image is locked — even a
local OS account (`kwayrdc`) cannot read the files without the passphrase.

### Setup on the Mac Mini (kwayrdc)

```bash
# 1. Choose a storage location (can be the data volume or an external drive):
sudo mkdir -p /Users/kwayrdc/kway-dmg-store
sudo chown kwayrdc /Users/kwayrdc/kway-dmg-store

# 2. Add to backend/.env:
#    DMG_ROOT=/Users/kwayrdc/kway-dmg-store
#    PROJECT_DATA_ROOT=/Users/kwayrdc/kway-project-data
#    DMG_SIZE_MB=4096

# 3. Restart backend — images will be created on next user login.
docker compose restart backend
```

### Recovery: retrieve a user's DMG passphrase

If a user's image needs to be opened by an admin (e.g. during account deletion):

```bash
# Find the user's UUID from DB:
psql $DATABASE_URL -c "SELECT id FROM users WHERE email = 'user@example.com';"

# Recover the passphrase via vault admin CLI:
va recover --type user_dmg_key --id <user_uuid>

# The output is the raw passphrase bytes (base64-encoded by hdiutil expectations).
# Mount manually:
hdiutil attach -stdinpass -mountpoint /tmp/recover-mount \
    <DMG_ROOT>/<user_uuid>.sparseimage
# Enter the passphrase when prompted, or pipe it:
echo "<recovered_passphrase>" | hdiutil attach -stdinpass \
    -mountpoint /tmp/recover-mount <DMG_ROOT>/<user_uuid>.sparseimage
```

### Scenario S6: Delete a user account

1. Log the user out via the app (triggers `hdiutil detach`).
2. Run `va recover --type user_dmg_key --id <uuid>` and archive plaintext if needed.
3. `va list` then purge: `psql … "DELETE FROM vault_ciphertexts WHERE object_id = '<uuid>' AND object_type = 'user_dmg_key';"`.
4. Delete the image file: `rm <DMG_ROOT>/<uuid>.sparseimage`.

---

## Emergency contacts

_(Fill in your on-call rotation here)_

| Role | Contact |
|---|---|
| Platform owner | @kway-rnd-infra |
| Security lead | — |
