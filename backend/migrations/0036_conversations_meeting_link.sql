-- Phase 4 (AI grounding): meeting flow needs to pass its transcript +
-- project context through the same context firewall as chat. The
-- firewall's audit table (agent_context_audit_logs.conversation_id) has
-- a NOT NULL FK to conversations(id), but meetings have no conversation.
--
-- Additive-only (project policy): add a NULLABLE meeting_id link column
-- to conversations. Existing chat conversations keep meeting_id = NULL
-- and behave exactly as before. A meeting's AI activity (minutes
-- generation grounding) is anchored to one dedicated conversation row
-- so the firewall FK is satisfied and the audit trail is queryable per
-- meeting. Nothing is modified or removed.

ALTER TABLE conversations
    ADD COLUMN IF NOT EXISTS meeting_id UUID
        REFERENCES meetings(id) ON DELETE CASCADE;

-- At most one grounding conversation per meeting (lets the get-or-create
-- helper use ON CONFLICT safely). Partial: chat rows (meeting_id NULL)
-- are unaffected.
CREATE UNIQUE INDEX IF NOT EXISTS uq_conversations_meeting
    ON conversations(meeting_id)
    WHERE meeting_id IS NOT NULL;
