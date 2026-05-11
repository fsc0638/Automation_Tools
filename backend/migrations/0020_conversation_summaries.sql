-- Per-conversation summary cache.
-- Project-level summary already exists in project_memory_summaries, but it
-- aggregates across every conversation in the project, so individual
-- conversations have no quick "what was this about" surface in the UI.
--
-- This table stores a lightweight summary computed after each turn, plus
-- a small bag of highlights and keywords used as a search/filter hint
-- on the conversation list.

CREATE TABLE conversation_summaries (
    conversation_id       UUID PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
    summary               TEXT NOT NULL,
    highlights            JSONB NOT NULL DEFAULT '[]'::JSONB,
    keywords              TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    source_message_count  INTEGER NOT NULL DEFAULT 0,
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Most lookups are "give me the summary for THIS conversation", which the
-- PK already handles. Add a GIN index on keywords for the future "find
-- conversations tagged X" query path.
CREATE INDEX idx_conv_summaries_keywords ON conversation_summaries USING GIN (keywords);
