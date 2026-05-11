-- Agent Data Policy controls how much project context a user-managed agent may receive.
-- Defaults preserve existing custom-agent behavior while making the policy explicit and auditable.

ALTER TABLE agent_profiles
    ADD COLUMN allowed_classification_max TEXT NOT NULL DEFAULT 'confidential'
        CHECK (allowed_classification_max IN ('public','internal','confidential','restricted','secret')),
    ADD COLUMN allow_code_context BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN allow_project_memory BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN allow_conversation_history BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN require_redaction BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN external_processing_allowed BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN retention_policy TEXT NOT NULL DEFAULT 'provider_default'
        CHECK (retention_policy IN ('none','session','provider_default'));

CREATE INDEX idx_ap_data_policy_classification ON agent_profiles(allowed_classification_max);
