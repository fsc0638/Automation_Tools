-- Multi-source workspaces (user decision 2026-05-19): one workspace may
-- aggregate MANY local folders and MANY Git repositories.
--
-- Additive only (project rule): a new child table. The existing single
-- source columns on `projects` (source_type / source_path / local_path
-- / git_identity_id / default_branch) are LEFT UNTOUCHED and continue
-- to act as the "primary / legacy" source so every current code path
-- (build_project_scope, project_root_path, single-source callers) keeps
-- working byte-identically. project_sources is the canonical list going
-- forward; the multi-source indexer/freshen iterate it, falling back to
-- the legacy columns when a project has no rows here yet.

CREATE TABLE IF NOT EXISTS project_sources (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind            TEXT NOT NULL DEFAULT 'local'
                        CHECK (kind IN ('local', 'git', 'upload')),
    -- local folder path OR git clone/browse URL
    source_path     TEXT NOT NULL,
    -- on-disk clone dir for git sources (NULL for local)
    local_path      TEXT,
    git_identity_id UUID REFERENCES git_identities(id) ON DELETE SET NULL,
    default_branch  TEXT,
    -- short display name; also used to namespace indexed file paths so
    -- two sources with the same relative path don't collide
    label           TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_project_sources_project
    ON project_sources(project_id);

-- Backfill: every existing project that has a real source becomes its
-- own first project_sources row, so behaviour is unchanged on day one
-- (the workspace already "has" exactly the source it has today).
INSERT INTO project_sources
    (project_id, kind, source_path, local_path, git_identity_id, default_branch, label)
SELECT p.id,
       p.source_type,
       p.source_path,
       p.local_path,
       p.git_identity_id,
       p.default_branch,
       p.name
FROM projects p
WHERE COALESCE(TRIM(p.source_path), '') <> ''
  AND NOT EXISTS (
      SELECT 1 FROM project_sources s WHERE s.project_id = p.id
  );
