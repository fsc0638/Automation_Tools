-- Workspace file registry: owner/workspace/project metadata for every
-- uploaded file, Git working copy root, meeting file, and AI artifact.
--
-- This does not move bytes. It registers existing storage paths so ACL,
-- classification, encryption state, versioning, and audit can be enforced
-- consistently by API and Agent task code.

CREATE TABLE IF NOT EXISTS workspace_files (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    owner_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    source_type TEXT NOT NULL CHECK (source_type IN ('upload','git','agent_artifact','portal','manual','meeting_file')),
    logical_path TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    classification TEXT NOT NULL DEFAULT 'internal'
        CHECK (classification IN ('public','internal','confidential','secret')),
    encryption_state TEXT NOT NULL DEFAULT 'dmg'
        CHECK (encryption_state IN ('dmg','vault','plaintext_dev')),
    content_hash TEXT,
    size_bytes BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_workspace_files_owner
    ON workspace_files(owner_user_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_workspace_files_workspace
    ON workspace_files(workspace_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_workspace_files_project
    ON workspace_files(project_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_workspace_files_storage_path
    ON workspace_files(storage_path);

CREATE TABLE IF NOT EXISTS file_versions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    file_id UUID NOT NULL REFERENCES workspace_files(id) ON DELETE CASCADE,
    version INT NOT NULL,
    content_hash TEXT NOT NULL,
    size_bytes BIGINT NOT NULL DEFAULT 0,
    storage_path TEXT NOT NULL,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(file_id, version)
);

CREATE INDEX IF NOT EXISTS idx_file_versions_file
    ON file_versions(file_id, version DESC);

CREATE TABLE IF NOT EXISTS file_access_audit (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    file_id UUID NOT NULL REFERENCES workspace_files(id) ON DELETE CASCADE,
    actor_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    operation TEXT NOT NULL CHECK (operation IN (
        'upload','download','read','write','delete','agent_read','agent_write','git_clone','git_pull','git_checkout'
    )),
    agent_name TEXT,
    task_id UUID,
    ip_addr TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_file_access_audit_file
    ON file_access_audit(file_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_file_access_audit_actor
    ON file_access_audit(actor_user_id, created_at DESC);

COMMENT ON TABLE workspace_files IS
    'Registry for user-scoped files stored on the Mac Mini: uploads, Git working copies, meeting files, portal data, and agent artifacts.';
COMMENT ON COLUMN workspace_files.encryption_state IS
    'dmg = protected by per-user mounted sparse image; vault = encrypted object in vault_ciphertexts; plaintext_dev = local/dev only.';
