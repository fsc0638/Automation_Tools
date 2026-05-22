-- Agent task broker: durable queue for autonomous agents running against
-- user/workspace/project scoped context.
--
-- The broker is intentionally metadata-first. It stores input/output envelopes,
-- lease state, and audit events; actual file access still goes through
-- workspace_files + vault/session-key gates.

CREATE TABLE IF NOT EXISTS agent_tasks (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    owner_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    requested_by UUID REFERENCES users(id) ON DELETE SET NULL,
    task_type TEXT NOT NULL CHECK (task_type IN ('code_change','analysis','test','sync','maintenance','custom')),
    agent_name TEXT NOT NULL,
    priority INT NOT NULL DEFAULT 100,
    status TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued','leased','running','completed','failed','cancelled','expired')),
    input JSONB NOT NULL DEFAULT '{}'::jsonb,
    output JSONB,
    error TEXT,
    lease_owner TEXT,
    lease_expires_at TIMESTAMPTZ,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_agent_tasks_owner_status
    ON agent_tasks(owner_user_id, status, priority, created_at);
CREATE INDEX IF NOT EXISTS idx_agent_tasks_project_status
    ON agent_tasks(project_id, status, priority, created_at);
CREATE INDEX IF NOT EXISTS idx_agent_tasks_lease
    ON agent_tasks(status, lease_expires_at) WHERE status IN ('leased','running');

CREATE TABLE IF NOT EXISTS agent_task_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    task_id UUID NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
    actor_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    agent_name TEXT,
    event_type TEXT NOT NULL CHECK (event_type IN ('created','leased','heartbeat','completed','failed','cancelled','expired')),
    note TEXT,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_agent_task_events_task
    ON agent_task_events(task_id, created_at DESC);

COMMENT ON TABLE agent_tasks IS
    'Durable task broker queue for local/private agents with explicit owner, workspace, project, lease, and audit metadata.';
COMMENT ON COLUMN agent_tasks.input IS
    'Structured task envelope. Should contain references/IDs, not raw secrets. File reads must use workspace_files ACL checks.';
