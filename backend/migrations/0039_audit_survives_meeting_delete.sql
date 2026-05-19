-- TC-F backlog fix (compliance): deleting a meeting must NOT erase its
-- agent_context_audit_logs trail.
--
-- Root cause chain: migration 0036 added conversations.meeting_id with
-- ON DELETE CASCADE; agent_context_audit_logs.conversation_id (mig
-- 0016) is also ON DELETE CASCADE. So: delete meeting → cascade-delete
-- its grounding conversation → cascade-delete the audit rows. Field
-- tests showed meeting_minutes audit rows vanishing after a test
-- meeting was deleted.
--
-- Minimal single-point fix: relax ONLY the conversations.meeting_id
-- foreign key from CASCADE to SET NULL. Deleting a meeting now just
-- unlinks its conversation (meeting_id → NULL); the conversation row
-- and therefore every agent_context_audit_logs row survive. No column
-- is added/removed/retyped; no audit data is ever lost (this strictly
-- PRESERVES more data). The partial unique index from 0036 is
-- unaffected (it only constrains non-NULL meeting_id).

ALTER TABLE conversations
    DROP CONSTRAINT IF EXISTS conversations_meeting_id_fkey;

ALTER TABLE conversations
    ADD CONSTRAINT conversations_meeting_id_fkey
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE SET NULL;
