-- Each user gets a stable, random 32-byte salt used exclusively for
-- User KEK derivation: Argon2id(password, kek_salt) → User KEK.
--
-- The KEK itself is NEVER stored in the database. It is derived at login
-- and held only in server session memory for the duration of the session.
--
-- IMPORTANT: Changing a user's kek_salt invalidates ALL existing
-- 'user:<id>' wrappings for that user. Only do this if you are also
-- re-wrapping all their DEKs (password-change flow handles this).

CREATE EXTENSION IF NOT EXISTS pgcrypto;

ALTER TABLE users ADD COLUMN kek_salt BYTEA;

-- Backfill existing users with a fresh random salt.
UPDATE users SET kek_salt = gen_random_bytes(32) WHERE kek_salt IS NULL;

ALTER TABLE users ALTER COLUMN kek_salt SET NOT NULL;

-- New users automatically get a salt at INSERT time.
ALTER TABLE users ALTER COLUMN kek_salt SET DEFAULT gen_random_bytes(32);
