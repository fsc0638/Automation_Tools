-- P1: extra fields + status audit log for project_tasks.
-- Goal: enough metadata for engineers (Dev/QA), PMs to manage and review,
-- plus an immutable transition log so status changes are auditable.

ALTER TABLE project_tasks
    ADD COLUMN assignee            TEXT,                              -- free-form: username, email, "OpenClaw", "QA team"
    ADD COLUMN due_date             DATE,                              -- day resolution; NULL = no deadline
    ADD COLUMN test_plan            TEXT,                              -- how QA verifies this task
    ADD COLUMN rollback_plan        TEXT,                              -- how to back the change out
    ADD COLUMN definition_of_done   TEXT,                              -- explicit DoD, separate from acceptance_criteria
    ADD COLUMN labels               TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[];  -- e.g. {"frontend","security","tech-debt"}

CREATE INDEX idx_pt_assignee ON project_tasks(project_id, assignee, status, due_date);
CREATE INDEX idx_pt_labels   ON project_tasks USING GIN (labels);
CREATE INDEX idx_pt_due_date ON project_tasks(project_id, due_date) WHERE due_date IS NOT NULL;

-- Append-only log of status transitions. Inserted server-side whenever
-- the status column actually changes (see api/tasks.rs::update_task).
CREATE TABLE task_status_history (
    id           UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    task_id      UUID NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    from_status  TEXT,                                  -- NULL on initial create
    to_status    TEXT NOT NULL,
    changed_by   UUID REFERENCES users(id) ON DELETE SET NULL,
    note         TEXT,
    changed_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_tsh_task_id ON task_status_history(task_id, changed_at DESC);
