-- Agent context authorization boundary.
--
-- This formalizes the boundary between project/workspace ACLs, workspace file
-- classification, and outbound agent context. The runtime firewall now consults
-- workspace_files classification before sending file blocks to agents; this
-- migration records that policy surface for audit/reporting.

CREATE TABLE IF NOT EXISTS agent_context_authorization_rules (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    organization_id UUID REFERENCES organizations(id) ON DELETE CASCADE,
    workspace_id UUID REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id UUID REFERENCES projects(id) ON DELETE CASCADE,
    agent_name TEXT,
    max_classification TEXT NOT NULL DEFAULT 'confidential'
        CHECK (max_classification IN ('public','internal','confidential','restricted','secret')),
    allow_code_context BOOLEAN NOT NULL DEFAULT TRUE,
    allow_project_memory BOOLEAN NOT NULL DEFAULT TRUE,
    allow_conversation_history BOOLEAN NOT NULL DEFAULT TRUE,
    require_redaction BOOLEAN NOT NULL DEFAULT TRUE,
    external_processing_allowed BOOLEAN NOT NULL DEFAULT TRUE,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_agent_context_auth_project
    ON agent_context_authorization_rules(project_id, agent_name);
CREATE INDEX IF NOT EXISTS idx_agent_context_auth_workspace
    ON agent_context_authorization_rules(workspace_id, agent_name);

ALTER TABLE agent_context_audit_logs
    ADD COLUMN IF NOT EXISTS registry_enforced BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS authorization_boundary_version TEXT NOT NULL DEFAULT 'workspace_files_v1';

COMMENT ON TABLE agent_context_authorization_rules IS
    'Optional scoped policy overrides for outbound agent context. Runtime still applies the most restrictive effective agent data policy.';
COMMENT ON COLUMN agent_context_audit_logs.registry_enforced IS
    'True when outbound context was checked against workspace_files registry classification in addition to path/text DLP.';
