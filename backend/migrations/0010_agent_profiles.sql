-- User-managed LLM agent profiles. API keys are encrypted at rest with TokenCipher.

CREATE TABLE agent_profiles (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    provider TEXT NOT NULL CHECK (provider IN ('openai', 'openai_compatible', 'gemini', 'anthropic')),
    model TEXT NOT NULL,
    base_url TEXT,
    role_prompt TEXT NOT NULL DEFAULT '',
    api_key TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_agent_profiles_user_id ON agent_profiles(user_id, updated_at DESC);
