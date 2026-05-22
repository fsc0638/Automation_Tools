-- Add ai_description to vault_secrets for AI agent context injection.
--
-- Users fill this field to tell the AI agent what each secret is for and
-- when / why to use it.  The description is injected as a system message
-- into every conversation so the agent can decide autonomously which
-- credential to use — without ever seeing the plaintext value.
--
-- Examples:
--   "Use this GitHub PAT when cloning or pushing to any private Kway repo."
--   "Staging DB password — use when the user asks about staging environment data."
--   "OpenAI API key for direct API calls when Hermes is unavailable."

ALTER TABLE vault_secrets
    ADD COLUMN ai_description TEXT NOT NULL DEFAULT '';
