-- Strict User KEK gate for git/sensitive file operations.
--
-- From this migration forward, normal API paths must encrypt/decrypt git
-- credentials only with an active in-RAM User KEK. The System KEK remains
-- available only to explicit admin recovery tooling (`vault_admin`) and is not
-- a transparent fallback for git clone/pull/checkout or workspace file access.

ALTER TABLE git_identities
    ADD COLUMN IF NOT EXISTS kek_policy TEXT NOT NULL DEFAULT 'user_required'
        CHECK (kek_policy IN ('user_required'));

COMMENT ON COLUMN git_identities.kek_policy IS
    'user_required = normal API git operations require active User KEK; System KEK fallback is forbidden outside admin recovery tooling.';
