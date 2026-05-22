-- AgentK 對齊 (2/3) — meeting_notes table.
--
-- Adds three JSONB columns so the latest version of a meeting's notes
-- can carry AgentK's records-aggregate shape (action_items + ai_job_ids
-- + task_ids) without giving up our existing versioned `meeting_notes`
-- design. Existing columns (summary / decisions / risks /
-- transcript_excerpts) are untouched.
--
-- Per docs/agentk-fusion/fusion-plan.md §2.2 (approved 2026-05-15).

ALTER TABLE meeting_notes
    ADD COLUMN IF NOT EXISTS action_items JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS ai_job_ids   JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS task_ids     JSONB NOT NULL DEFAULT '[]'::jsonb;

COMMENT ON COLUMN meeting_notes.action_items IS
    'AgentK 對齊：[{title, description?, assignee_user_id?, source?}]。與 risks / decisions 同層級。';
COMMENT ON COLUMN meeting_notes.ai_job_ids   IS
    'AgentK 對齊：產生此 note 的 AI job ID 列表（未來接 ai_gateway 時用，目前可空）。';
COMMENT ON COLUMN meeting_notes.task_ids     IS
    'AgentK 對齊：action_items 同步出去的 project_tasks ID 快取（便利讀取）。實際 task mutation 仍走 meeting_task_impacts。';
