-- Bucket B: cross-project surfaces.
--
-- B7: agent profile labels for grouping (e.g. "backend", "design").
-- B3: epics — user-scoped milestone buckets that can group tasks
--     across multiple projects.
-- B6: shared memory snippets — pinned facts / decisions that can opt
--     into being included in any of a user's projects.

-- ----------------------------------------------------------------
-- B7  agent_profiles.labels
-- ----------------------------------------------------------------
ALTER TABLE agent_profiles
    ADD COLUMN labels TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[];
CREATE INDEX idx_ap_labels ON agent_profiles USING GIN (labels);

-- ----------------------------------------------------------------
-- B3  epics + project_tasks.epic_id
-- ----------------------------------------------------------------
CREATE TABLE epics (
    id          UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id     UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT,
    color       TEXT,         -- hex like '#0050A0' for chip rendering
    status      TEXT NOT NULL DEFAULT 'planned'
                   CHECK (status IN ('planned','active','done','archived')),
    target_date DATE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_epics_user ON epics(user_id, created_at DESC);

ALTER TABLE project_tasks
    ADD COLUMN epic_id UUID REFERENCES epics(id) ON DELETE SET NULL;
CREATE INDEX idx_pt_epic ON project_tasks(epic_id) WHERE epic_id IS NOT NULL;

-- ----------------------------------------------------------------
-- B6  shared_memory_notes
-- ----------------------------------------------------------------
CREATE TABLE shared_memory_notes (
    id           UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    body         TEXT NOT NULL,
    tags         TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    -- Visibility scope. Empty array = visible in ALL of the user's
    -- projects (the "global" mode); non-empty = whitelist of project
    -- ids the note opts into. ARRAY rather than a join table because
    -- a note is rarely shared with more than a handful of projects.
    scope_projects UUID[] NOT NULL DEFAULT ARRAY[]::UUID[],
    pinned       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_smn_user ON shared_memory_notes(user_id, updated_at DESC);
CREATE INDEX idx_smn_tags ON shared_memory_notes USING GIN (tags);
CREATE INDEX idx_smn_scope ON shared_memory_notes USING GIN (scope_projects);
