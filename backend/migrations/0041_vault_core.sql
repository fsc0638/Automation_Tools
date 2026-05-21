-- Vault infrastructure for envelope encryption (Option B: User KEK + System Recovery).
-- Decided 2026-05-21.
--
-- Key hierarchy:
--   Argon2id(user_password, users.kek_salt)  → User KEK  (session RAM only, never stored)
--   GIT_TOKEN_ENCRYPTION_KEY env var          → System KEK (recovery path, audit-logged)
--
--   random DEK (per object) ─── AES-256-GCM ──► ciphertext  (vault_ciphertexts)
--                           └── wrap with User KEK   ──► vault_key_wrappings kek_alias='user:<uuid>'
--                           └── wrap with System KEK ──► vault_key_wrappings kek_alias='system_v1'
--
-- Each sealed object gets exactly TWO wrapping rows so the owner can
-- always decrypt (User KEK path) and an admin can recover via the CLI
-- (System KEK path, with mandatory audit log entry).

-- System KEK alias registry (user KEKs are derived, never stored here).
CREATE TABLE vault_keys (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    alias       TEXT NOT NULL UNIQUE,   -- 'system_v1', 'system_v2', ...
    status      TEXT NOT NULL DEFAULT 'active'
                CHECK (status IN ('active', 'retired')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    retired_at  TIMESTAMPTZ
);

INSERT INTO vault_keys (alias) VALUES ('system_v1');

-- Per-object wrapped DEK storage.
-- kek_alias: 'system_v1'     → wrapped with System KEK (recovery)
--            'user:<user_id>' → wrapped with User KEK  (normal access)
-- One sealed object → two rows here, one per KEK kind.
CREATE TABLE vault_key_wrappings (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    object_type TEXT NOT NULL,           -- 'vault_secret' | 'vault_file' | 'git_token' | ...
    object_id   UUID NOT NULL,
    kek_alias   TEXT NOT NULL,
    wrapped_dek TEXT NOT NULL,           -- base64(TokenCipher.encrypt(base64(dek_bytes)))
    wrap_alg    TEXT NOT NULL DEFAULT 'aes256gcm_v1',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (object_type, object_id, kek_alias)
);

CREATE INDEX idx_vault_wrappings_lookup
    ON vault_key_wrappings (object_type, object_id);
CREATE INDEX idx_vault_wrappings_kek
    ON vault_key_wrappings (kek_alias);

-- Encrypted object ciphertext.
-- For objects <= 256 KB: ciphertext stored inline (BYTEA column).
-- For larger objects:    ciphertext = NULL, storage_path = encrypted file on disk.
CREATE TABLE vault_ciphertexts (
    id            UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    object_type   TEXT    NOT NULL,
    object_id     UUID    NOT NULL UNIQUE,
    cipher_alg    TEXT    NOT NULL DEFAULT 'aes256gcm_v1',
    nonce         BYTEA   NOT NULL,             -- 12 bytes, random per seal
    ciphertext    BYTEA,                        -- NULL ↔ see storage_path
    storage_path  TEXT,                         -- on-disk encrypted file path
    aad           TEXT    NOT NULL,             -- '{object_type}:{object_id}' (anti-substitution)
    plain_size    BIGINT,                       -- original plaintext bytes
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_vault_ciphertexts_lookup
    ON vault_ciphertexts (object_type, object_id);

-- Credential / secret vault — metadata only.
-- The actual secret lives in vault_ciphertexts (object_type='vault_secret').
CREATE TABLE vault_secrets (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    label       TEXT NOT NULL,
    secret_type TEXT NOT NULL DEFAULT 'password'
                CHECK (secret_type IN ('password', 'token', 'certificate', 'note', 'device')),
    username    TEXT,
    url         TEXT,
    note        TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_vault_secrets_user
    ON vault_secrets (user_id, updated_at DESC);

-- Append-only audit log for every vault operation.
-- kek_path: 'user'            → normal User KEK access
--           'system_recovery' → admin used System KEK (high-visibility event)
CREATE TABLE vault_audit_log (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    actor_id    UUID REFERENCES users(id) ON DELETE SET NULL,
    object_type TEXT NOT NULL,
    object_id   UUID NOT NULL,
    operation   TEXT NOT NULL
                CHECK (operation IN ('encrypt','decrypt','rewrap','delete','recovery','reveal')),
    kek_path    TEXT NOT NULL DEFAULT 'user'
                CHECK (kek_path IN ('user', 'system_recovery')),
    reason      TEXT NOT NULL DEFAULT 'normal',
    ip_addr     TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_vault_audit_object
    ON vault_audit_log (object_type, object_id, created_at DESC);
CREATE INDEX idx_vault_audit_actor
    ON vault_audit_log (actor_id, created_at DESC);
CREATE INDEX idx_vault_audit_operation
    ON vault_audit_log (operation, created_at DESC);
