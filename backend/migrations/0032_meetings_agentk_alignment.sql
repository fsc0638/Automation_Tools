-- AgentK 對齊 (1/3) — meetings table.
--
-- 純新增；既有欄位（含 portal_*、external_id 等）完全不動。新欄位用
-- 途與既有相似時並存（例：notification_note 留為通知文字、新加
-- description 給長介紹；portal_booked_at 留為凱衛專用、新加 sync_status
-- / last_synced_at 給通用同步狀態）。
--
-- Per docs/agentk-fusion/fusion-plan.md §2.1 (approved 2026-05-15).

ALTER TABLE meetings
    ADD COLUMN IF NOT EXISTS description        TEXT,
    ADD COLUMN IF NOT EXISTS is_locked          BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS join_url           TEXT,
    ADD COLUMN IF NOT EXISTS external_provider  TEXT,
    ADD COLUMN IF NOT EXISTS external_event_id  TEXT,
    ADD COLUMN IF NOT EXISTS external_event_url TEXT,
    ADD COLUMN IF NOT EXISTS sync_status        TEXT,
    ADD COLUMN IF NOT EXISTS last_synced_at     TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS updated_by_user_id UUID REFERENCES users(id) ON DELETE SET NULL;

-- Backfill: existing 'completed' rows should be locked by default. Future
-- transitions to 'completed' will set is_locked=TRUE in app code; this
-- catches the historical rows so the semantics start consistent.
UPDATE meetings SET is_locked = TRUE WHERE status = 'completed' AND is_locked = FALSE;

CREATE INDEX IF NOT EXISTS idx_meetings_is_locked
    ON meetings(is_locked) WHERE is_locked = TRUE;

COMMENT ON COLUMN meetings.description       IS
    'AgentK 對齊：會議長描述。與既有 notification_note 不同 — notification_note 是寄給與會人的通知文字，description 是會議本身的介紹。兩者並存。';
COMMENT ON COLUMN meetings.is_locked         IS
    'AgentK 對齊：獨立於 status 的鎖。status=completed 時 app 端會自動設為 TRUE；admin/owner 可清除（reopen 流程）。';
COMMENT ON COLUMN meetings.join_url          IS
    'AgentK 對齊：線上會議直接連結（Webex / Teams / Meet）。線上 toggle 時填。';
COMMENT ON COLUMN meetings.external_provider IS
    'AgentK 對齊：外部 provider symbol（webex / teams / meet / kway-portal）。與既有 external_id（opaque key like "portal:CODE-DATE-HHMM"）並存。';
COMMENT ON COLUMN meetings.external_event_id IS
    'AgentK 對齊：外部系統的 event id（Webex/Teams 會回；凱衛 portal 沒回但欄位保留）。';
COMMENT ON COLUMN meetings.external_event_url IS
    'AgentK 對齊：外部 provider 頁面連結（可點擊回原系統）。';
COMMENT ON COLUMN meetings.sync_status       IS
    'AgentK 對齊：通用同步狀態（pending / synced / failed）。與既有 portal_booked_at / portal_book_error（凱衛專用）並存。';
COMMENT ON COLUMN meetings.last_synced_at    IS
    'AgentK 對齊：最後同步時間（任何 provider）。與 portal_booked_at 並存 — 後者只記凱衛成功。';
COMMENT ON COLUMN meetings.updated_by_user_id IS
    'AgentK 對齊：最後修改者。既有 updated_at 已有時間，這裡補審計人。';
