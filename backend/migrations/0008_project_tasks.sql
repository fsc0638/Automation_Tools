-- Persistent task list per project. Populated by the Roadmap tab —
-- either manually or extracted from an Agent Patch / Roadmap reply.

CREATE TABLE project_tasks (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    why TEXT,
    affected_files TEXT[],
    acceptance_criteria TEXT,
    estimated_effort TEXT,                                                 -- 'small' | 'medium' | 'large'
    priority TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN ('low','medium','high','critical')),
    status TEXT NOT NULL DEFAULT 'todo' CHECK (status IN ('todo','in-progress','done','cancelled')),
    source_message_id UUID REFERENCES messages(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_pt_project_status ON project_tasks(project_id, status, created_at DESC);
