-- AgentK 對齊：ai_jobs persistence audit.
--
-- 目前 meeting_notes.ai_job_ids 是空陣列 — 因為沒有對應的表來查 job
-- 細節。這張表補上：每次 LLM call 寫一筆，回 id 給呼叫端，呼叫端把
-- id 塞進對應 record 的 ai_job_ids JSONB 陣列。
--
-- 簡化版：只有 meeting minutes 一個 caller。未來如果 conversation
-- summary / project summary 也要 audit，再加 `kind` 列舉。
--
-- 不收訊息原文（隱私），只記 metadata + 結構化結果摘要。

CREATE TABLE IF NOT EXISTS ai_jobs (
    id              UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    kind            TEXT NOT NULL,       -- e.g. 'meeting_minutes'
    provider        TEXT NOT NULL,       -- e.g. 'hermes' / 'openclaw' / 'gemini'
    model           TEXT NOT NULL,
    requested_by    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Optional FK to the resource the job served. Meeting minutes use
    -- this; other kinds may leave it NULL.
    meeting_id      UUID REFERENCES meetings(id) ON DELETE SET NULL,
    conversation_id UUID,                -- forward-compat; no FK because
                                          -- conversations table is owned
                                          -- by another module
    status          TEXT NOT NULL,        -- 'success' | 'failed' | 'timeout'
    input_chars     INT,
    output_chars    INT,
    duration_ms     INT,
    error           TEXT,
    /* prompt_hash gives us a fingerprint to dedupe / cache later without
       storing raw text. SHA-256 of the user-prompt body, hex. */
    prompt_hash     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ai_jobs_meeting_id ON ai_jobs(meeting_id);
CREATE INDEX IF NOT EXISTS idx_ai_jobs_requested_by ON ai_jobs(requested_by);
CREATE INDEX IF NOT EXISTS idx_ai_jobs_kind_created ON ai_jobs(kind, created_at DESC);

COMMENT ON TABLE ai_jobs IS
    'AgentK-aligned audit log for LLM invocations. One row per chat() call.';
COMMENT ON COLUMN ai_jobs.kind IS
    'Caller semantic. Currently: meeting_minutes. Add more as we wire other callers.';
COMMENT ON COLUMN ai_jobs.prompt_hash IS
    'SHA-256 hex of the user prompt body. Lets us look up cached results without storing the raw prompt.';
