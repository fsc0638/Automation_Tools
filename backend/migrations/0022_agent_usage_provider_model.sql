ALTER TABLE agent_usage_events
    ADD COLUMN provider TEXT,
    ADD COLUMN model TEXT;

CREATE INDEX idx_aue_provider ON agent_usage_events(provider, created_at);
CREATE INDEX idx_aue_model ON agent_usage_events(model, created_at);
