-- Relax the vault_secrets.secret_type CHECK constraint.
--
-- The original constraint only allowed a fixed enum
-- ('password' | 'token' | 'certificate' | 'note' | 'device'), but the UI
-- exposes a free-text field so users can enter e.g. 'api_key', 'ssh_key',
-- 'service_account', etc.  Drop the constraint and keep the column NOT NULL
-- with a sensible default.

ALTER TABLE vault_secrets
    DROP CONSTRAINT IF EXISTS vault_secrets_secret_type_check;
