-- P3: collaborative review surfaces — comments thread per task,
-- sprints for milestone grouping, and a column on project_tasks to
-- bind tasks to a sprint.

-- ------------------------------------------------------------------
-- Task comments
-- ------------------------------------------------------------------
CREATE TABLE task_comments (
    id         UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    task_id    UUID NOT NULL REFERENCES project_tasks(id) ON DELETE CASCADE,
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content    TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_tc_task_id ON task_comments(task_id, created_at);

-- ------------------------------------------------------------------
-- Sprints (milestone buckets)
-- ------------------------------------------------------------------
CREATE TABLE sprints (
    id         UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    goal       TEXT,
    start_date DATE,
    end_date   DATE,
    status     TEXT NOT NULL DEFAULT 'planned'
                  CHECK (status IN ('planned','active','closed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_sp_project ON sprints(project_id, created_at DESC);

-- ------------------------------------------------------------------
-- Bind tasks to a sprint
-- ------------------------------------------------------------------
ALTER TABLE project_tasks
    ADD COLUMN sprint_id UUID REFERENCES sprints(id) ON DELETE SET NULL;

CREATE INDEX idx_pt_sprint ON project_tasks(sprint_id) WHERE sprint_id IS NOT NULL;
