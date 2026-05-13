-- Per-agent-call telemetry for the Insights / Cost / Health dashboards.
-- One row per chat_stream invocation (Debate produces multiple rows per turn).

CREATE TABLE agent_usage_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    conversation_id UUID NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    message_id UUID REFERENCES messages(id) ON DELETE SET NULL,
    agent TEXT NOT NULL,                         -- agent slug: 'openclaw' | 'hermes' | custom-agent id
    mode TEXT NOT NULL,                          -- 'openclaw' | 'hermes' | 'debate' | 'agent:<id>' | 'agents:<id1>,<id2>,...'
    phase TEXT,                                  -- 'round' | 'final' | 'chat' | 'quick_check' | NULL
    round_number INT,
    ttft_ms INT,                                 -- ms from start to first streamed chunk
    total_ms INT,                                -- ms from start to Done
    tokens_in INT,                               -- estimated input tokens (NULL if not computed)
    tokens_out INT,                              -- estimated output tokens (NULL if not computed)
    chars_out INT NOT NULL DEFAULT 0,
    has_consensus_marker BOOLEAN NOT NULL DEFAULT FALSE,
    has_file_citation BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_aue_project_id ON agent_usage_events(project_id, created_at);
CREATE INDEX idx_aue_conversation_id ON agent_usage_events(conversation_id, created_at);
CREATE INDEX idx_aue_mode ON agent_usage_events(mode, created_at);
CREATE INDEX idx_aue_agent ON agent_usage_events(agent, created_at);
