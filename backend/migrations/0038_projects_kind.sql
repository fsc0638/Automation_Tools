-- Phase 1 of the "專案 → 工作區" demotion (docs/workspace-kind/).
--
-- Additive only (project rule): add a workspace-kind discriminator and
-- an archive flag to `projects`. NOTHING is modified or removed; all
-- 12+ child tables, ~30 routes, the ACL functions, and the grounding
-- pipeline keep working byte-identically because every existing row
-- defaults to kind='code' (today's behaviour).
--
-- kind semantics:
--   code    — repo-backed workspace: git/clone/index/grounding (status quo)
--   admin   — 行政庶務 work area: no repo; todos/meetings/notes only
--   general — catch-all / personal / sandbox (same backend behaviour as admin)
-- Backend treats this as binary (is_code = kind='code'); admin vs
-- general is a UI-only category so finer classes can be added later
-- with zero backend change. Archive is a SEPARATE flag, not a kind
-- value (it is a state, not a type).

ALTER TABLE projects
    ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'code';

ALTER TABLE projects
    ADD COLUMN IF NOT EXISTS archived_at TIMESTAMPTZ;

-- Constrain values (named so it can be dropped/extended additively
-- later if more UI categories are introduced).
ALTER TABLE projects
    DROP CONSTRAINT IF EXISTS projects_kind_check;
ALTER TABLE projects
    ADD CONSTRAINT projects_kind_check
    CHECK (kind IN ('code', 'admin', 'general'));

-- Backfill per the 2026-05-19 decision:
--   code:    Automation_Tools / AgentK_develop / AgentK_FSC
--            (covered by the DEFAULT 'code' above — no action needed)
--   general: AI_Agent_Future (personal test), plus the leftover 'Test'
--            scratch workspace. Adjust freely later via the UI/API.
UPDATE projects
   SET kind = 'general'
 WHERE kind = 'code'
   AND name IN ('AI_Agent_Future', 'Test');

CREATE INDEX IF NOT EXISTS idx_projects_kind ON projects(kind);
