-- Attribute user-authored messages to the user who sent them.
--
-- Once projects can be shared (project_acl from mig 0019), multiple users
-- post into the same conversation. The chat UI needs to label each turn so
-- a collaborator can tell `kicl1143057` from `mm0389798`. We add a single
-- nullable column: assistant / system rows leave it NULL, user rows carry
-- the sender id.

ALTER TABLE messages
    ADD COLUMN user_id UUID REFERENCES users(id) ON DELETE SET NULL;

-- Backfill: every existing user message belongs to the conversation's
-- creator. That's not strictly accurate for any conversation that already
-- had multiple senders, but in practice the project sharing feature is
-- brand new and no such mixed-author history exists yet.
UPDATE messages m
SET user_id = c.user_id
FROM conversations c
WHERE c.id = m.conversation_id
  AND m.role = 'user'
  AND m.user_id IS NULL;

CREATE INDEX idx_messages_user_id ON messages(user_id);
