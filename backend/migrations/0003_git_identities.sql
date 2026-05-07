CREATE TABLE IF NOT EXISTS git_identities (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    provider TEXT NOT NULL DEFAULT 'generic',
    username TEXT NOT NULL,
    access_token TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_git_identities_user_id ON git_identities(user_id);

ALTER TABLE projects
    ADD COLUMN IF NOT EXISTS git_identity_id UUID REFERENCES git_identities(id) ON DELETE SET NULL;
