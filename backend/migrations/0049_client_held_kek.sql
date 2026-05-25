-- Client-Held KEK migration (Option B).
--
-- From this point onward, the User KEK is derived ON THE CLIENT
-- (browser / iOS) from the plaintext password + per-user kek_salt.
-- The server NEVER sees the plaintext password. Login payload is:
--
--   { email, auth_hash, user_kek }
--
-- where both auth_hash and user_kek are 32-byte Argon2id outputs,
-- domain-separated by their prepended labels:
--
--   auth_hash = Argon2id("kway-auth-v1::" || password, kek_salt)
--   user_kek  = Argon2id("kway-kek-v1::"  || password, kek_salt)
--
-- The server still applies a second Argon2 pass on auth_hash before
-- storing it in users.password_hash, so a DB leak still costs the
-- attacker an offline brute-force.
--
-- The user_kek is held in RAM (session_keys store) for the session
-- TTL only, then zeroized — same lifetime as before, but the source
-- of truth for derivation has moved off the server.
--
-- ── Data wipe ────────────────────────────────────────────────────────
-- The new auth protocol is incompatible with existing password_hash
-- rows (those were produced from raw passwords; new ones come from
-- client-derived auth_hash). Per project owner decision (2026-05-25),
-- we wipe and restart instead of writing a dual-path migration script.
-- CASCADE FK chains will purge refresh_tokens, vault_secrets,
-- vault_key_wrappings, vault_ciphertexts, project_sources, meetings,
-- and so on.

TRUNCATE TABLE users RESTART IDENTITY CASCADE;

-- ── Schema docs (no structural change) ───────────────────────────────

COMMENT ON COLUMN users.password_hash IS
    'Server-side Argon2id hash of the CLIENT-derived auth_hash (32-byte raw bytes, base64 over the wire). The plaintext password never reaches the server.';

COMMENT ON COLUMN users.kek_salt IS
    'Per-user 32-byte random salt for client-side Argon2id KEK + auth_hash derivation. Returned via GET /auth/kek-params before login.';
