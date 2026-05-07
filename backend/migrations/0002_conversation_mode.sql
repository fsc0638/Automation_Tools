ALTER TABLE conversations
ADD COLUMN IF NOT EXISTS mode TEXT NOT NULL DEFAULT 'openclaw'
CHECK (mode IN ('openclaw', 'hermes', 'debate'));

CREATE INDEX IF NOT EXISTS idx_conversations_project_mode
ON conversations(project_id, mode);
