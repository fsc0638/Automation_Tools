CREATE TABLE IF NOT EXISTS project_memory_summaries (
    project_id UUID PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    summary TEXT NOT NULL,
    source_message_count INTEGER NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_project_memory_summaries_updated_at
ON project_memory_summaries(updated_at DESC);
