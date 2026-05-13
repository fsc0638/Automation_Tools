-- #25: Widen conversations.mode to allow custom agent and custom debate encodings.
-- The mig 0002 CHECK constraint restricted mode to ('openclaw','hermes','debate'),
-- which forced the backend to store "openclaw" as a placeholder whenever a user
-- started a Gemini / Claude / Custom Debate conversation.  Dropping the constraint
-- lets normalize_mode pass through "agent:<uuid>" and "agents:<uuid>,..." strings
-- so conv.mode truthfully reflects which agent(s) the conversation uses.
ALTER TABLE conversations DROP CONSTRAINT IF EXISTS conversations_mode_check;
