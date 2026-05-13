-- Preserve agent_usage_events history across conversation/project deletion.
--
-- Until now both FKs went ON DELETE CASCADE, so deleting a conversation
-- (or its project) silently wiped every related cost / token / debate
-- metric row. That made the Cost tab and global Insights non-deterministic
-- — last month's spend could change as users tidied up old chats.
--
-- This migration switches both FKs to ON DELETE SET NULL and adds three
-- denormalized columns so reports can still show a human-readable label
-- for events whose conversation or project has since been deleted:
--   conversation_title_snapshot, project_name_snapshot, user_id
--
-- user_id is added to make the user_views.rs queries independent of
-- project ACL — once a project is deleted, user_can_access_project()
-- would otherwise hide that user's own historical spend from them.

-- 1) FKs: CASCADE → SET NULL
ALTER TABLE agent_usage_events
    DROP CONSTRAINT agent_usage_events_conversation_id_fkey,
    ADD  CONSTRAINT agent_usage_events_conversation_id_fkey
        FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE SET NULL;

ALTER TABLE agent_usage_events
    DROP CONSTRAINT agent_usage_events_project_id_fkey,
    ADD  CONSTRAINT agent_usage_events_project_id_fkey
        FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE SET NULL;

-- 2) Snapshot + ownership columns
ALTER TABLE agent_usage_events
    ADD COLUMN conversation_title_snapshot TEXT,
    ADD COLUMN project_name_snapshot       TEXT,
    ADD COLUMN user_id                     UUID REFERENCES users(id) ON DELETE SET NULL;

-- 3) Backfill from live joins. After this every existing row has snapshots
--    and a usable user_id even if its conv/project are later deleted.
UPDATE agent_usage_events e
SET conversation_title_snapshot = c.title,
    project_name_snapshot       = p.name,
    user_id                     = p.user_id
FROM conversations c, projects p
WHERE c.id = e.conversation_id
  AND p.id = e.project_id;

-- 4) FK columns must be nullable now that SET NULL can put NULL in them.
ALTER TABLE agent_usage_events
    ALTER COLUMN project_id      DROP NOT NULL,
    ALTER COLUMN conversation_id DROP NOT NULL;

-- 5) Lookups by owning user (replaces ACL-via-project for cost reports).
CREATE INDEX idx_aue_user_id ON agent_usage_events(user_id, created_at);
