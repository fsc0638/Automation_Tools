-- Organization / Workspace / Project ACL foundation.
-- Existing single-user projects are migrated into a personal organization + default workspace.

CREATE TABLE organizations (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    name TEXT NOT NULL,
    owner_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_org_owner ON organizations(owner_user_id);

CREATE TABLE organization_members (
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner','admin','member','viewer')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (organization_id, user_id)
);
CREATE INDEX idx_org_members_user ON organization_members(user_id);

CREATE TABLE workspaces (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_workspaces_org ON workspaces(organization_id);

CREATE TABLE workspace_members (
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner','admin','member','viewer')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (workspace_id, user_id)
);
CREATE INDEX idx_workspace_members_user ON workspace_members(user_id);

ALTER TABLE projects
    ADD COLUMN organization_id UUID REFERENCES organizations(id) ON DELETE SET NULL,
    ADD COLUMN workspace_id UUID REFERENCES workspaces(id) ON DELETE SET NULL;
CREATE INDEX idx_projects_org ON projects(organization_id);
CREATE INDEX idx_projects_workspace ON projects(workspace_id);

CREATE TABLE project_acl (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner','admin','editor','viewer')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, user_id)
);
CREATE INDEX idx_project_acl_user ON project_acl(user_id);

-- Bootstrap one personal org/workspace per current user.
INSERT INTO organizations (owner_user_id, name)
SELECT u.id, COALESCE(NULLIF(u.display_name, ''), u.email) || ' Personal Org'
FROM users u;

INSERT INTO organization_members (organization_id, user_id, role)
SELECT o.id, o.owner_user_id, 'owner'
FROM organizations o;

INSERT INTO workspaces (organization_id, name)
SELECT o.id, 'Default Workspace'
FROM organizations o;

INSERT INTO workspace_members (workspace_id, user_id, role)
SELECT w.id, o.owner_user_id, 'owner'
FROM workspaces w
JOIN organizations o ON o.id = w.organization_id;

UPDATE projects p
SET organization_id = o.id,
    workspace_id = w.id
FROM organizations o
JOIN workspaces w ON w.organization_id = o.id
WHERE o.owner_user_id = p.user_id
  AND p.organization_id IS NULL;

INSERT INTO project_acl (project_id, user_id, role)
SELECT id, user_id, 'owner'
FROM projects
ON CONFLICT DO NOTHING;

ALTER TABLE projects
    ALTER COLUMN organization_id SET NOT NULL,
    ALTER COLUMN workspace_id SET NOT NULL;

CREATE OR REPLACE FUNCTION access_role_rank(role_text TEXT)
RETURNS INTEGER
LANGUAGE SQL
IMMUTABLE
AS $$
    SELECT CASE role_text
        WHEN 'owner' THEN 40
        WHEN 'admin' THEN 30
        WHEN 'editor' THEN 20
        WHEN 'member' THEN 20
        WHEN 'viewer' THEN 10
        ELSE 0
    END;
$$;

CREATE OR REPLACE FUNCTION user_can_access_project(
    p_project_id UUID,
    p_user_id UUID,
    p_min_role TEXT DEFAULT 'viewer'
)
RETURNS BOOLEAN
LANGUAGE SQL
STABLE
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM projects p
        LEFT JOIN project_acl pa
          ON pa.project_id = p.id AND pa.user_id = p_user_id
        LEFT JOIN organization_members om
          ON om.organization_id = p.organization_id AND om.user_id = p_user_id
        LEFT JOIN workspace_members wm
          ON wm.workspace_id = p.workspace_id AND wm.user_id = p_user_id
        WHERE p.id = p_project_id
          AND (
            p.user_id = p_user_id
            OR access_role_rank(pa.role) >= access_role_rank(p_min_role)
            OR access_role_rank(om.role) >= access_role_rank(p_min_role)
            OR access_role_rank(wm.role) >= access_role_rank(p_min_role)
          )
    );
$$;
