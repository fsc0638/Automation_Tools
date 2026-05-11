-- E1: generic audit log for task field changes beyond status.
-- task_status_history already tracks status transitions (P1). This
-- table captures everything else (sprint, assignee, labels, priority,
-- due_date, …) so PM can answer "who changed X to Y, when?".

CREATE TABLE task_audit_log (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    task_id     UUID NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    -- "sprint" | "assignee" | "labels" | "priority" | "due_date"
    -- | "linked_pr_url" | "linked_commit_sha" | "depends_on"
    -- (status is in task_status_history, not duplicated here)
    field       TEXT NOT NULL,
    old_value   TEXT,     -- nullable; serialised JSON for arrays / dates
    new_value   TEXT,
    actor_id    UUID REFERENCES users(id) ON DELETE SET NULL,
    changed_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_tal_task_changed ON task_audit_log(task_id, changed_at DESC);
CREATE INDEX idx_tal_actor       ON task_audit_log(actor_id);
