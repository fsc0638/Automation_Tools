-- Context Firewall + DLP audit trail.
-- Records the sanitized outbound context envelope sent to Hermes/OpenClaw/custom agents.
-- Raw context is intentionally not stored here; only hash, file list, blocked items, and counters.

CREATE TABLE agent_context_audit_logs (
    id                    UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id               UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project_id            UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    conversation_id       UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    agent_mode            TEXT NOT NULL,
    outbound_context_hash TEXT NOT NULL,
    included_files        JSONB NOT NULL DEFAULT '[]'::JSONB,
    blocked_items         JSONB NOT NULL DEFAULT '[]'::JSONB,
    redacted_count        INTEGER NOT NULL DEFAULT 0,
    classification_max    TEXT NOT NULL DEFAULT 'public'
        CHECK (classification_max IN ('public','internal','confidential','restricted','secret')),
    token_estimate        INTEGER NOT NULL DEFAULT 0,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_acal_project_created ON agent_context_audit_logs(project_id, created_at DESC);
CREATE INDEX idx_acal_conversation_created ON agent_context_audit_logs(conversation_id, created_at DESC);
CREATE INDEX idx_acal_classification ON agent_context_audit_logs(classification_max);
