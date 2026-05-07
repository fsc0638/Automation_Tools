-- Switching git_identities.access_token storage from plaintext to AES-GCM ciphertext.
-- Existing plaintext rows cannot be decrypted by the new code, so clear them.
-- Users will re-enter Git identities through the UI.

DELETE FROM git_identities;
