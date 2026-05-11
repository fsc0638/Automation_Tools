-- P2: structured acceptance criteria, PR/commit linkage, dependencies,
-- and an attempts log so each "send to agent" dispatch is traceable.

ALTER TABLE project_tasks
    ADD COLUMN acceptance_criteria_v2 JSONB,                          -- { tests:[], commands:[], diff_hints:[], behavior:[] }
    ADD COLUMN linked_pr_url           TEXT,
    ADD COLUMN linked_commit_sha       TEXT,
    ADD COLUMN depends_on              UUID[] NOT NULL DEFAULT ARRAY[]::UUID[];

CREATE INDEX idx_pt_depends_on ON project_tasks USING GIN (depends_on);

-- Each row = one "agent execution" of a task. Bound to a conversation so
-- the user can re-open the chat that the dispatch produced.
CREATE TABLE task_attempts (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    task_id         UUID NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    mode            TEXT NOT NULL,                                    -- "openclaw" | "hermes" | "debate" | "agent:<id>" | "agents:<ids>"
    status          TEXT NOT NULL DEFAULT 'pending'
                       CHECK (status IN ('pending','running','complete','failed','cancelled')),
    dispatched_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    note            TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_ta_task_id ON task_attempts(task_id, created_at DESC);
CREATE INDEX idx_ta_conv_id ON task_attempts(conversation_id);
