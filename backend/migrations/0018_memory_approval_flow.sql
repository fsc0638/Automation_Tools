-- Memory Approval Flow.
-- AI-generated durable project memory is now staged as a candidate.
-- Approved candidates are applied to project_memory_summaries by an authenticated user/action.

CREATE TABLE project_memory_candidates (
    id                   UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id           UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    candidate_type       TEXT NOT NULL DEFAULT 'project_summary'
        CHECK (candidate_type IN ('project_summary')),
    proposed_content     TEXT NOT NULL,
    source_message_count INTEGER NOT NULL DEFAULT 0,
    source_context_hash  TEXT NOT NULL,
    status               TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','approved','rejected')),
    review_note          TEXT,
    reviewed_by          UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reviewed_at          TIMESTAMPTZ,
    applied_at           TIMESTAMPTZ
);

CREATE INDEX idx_pmc_project_status_created ON project_memory_candidates(project_id, status, created_at DESC);
CREATE INDEX idx_pmc_context_hash ON project_memory_candidates(project_id, source_context_hash);
