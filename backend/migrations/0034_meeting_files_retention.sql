-- AgentK 對齊 (3/3) — meeting_files retention.
--
-- AgentK 用一個通用 `assets` table 加 soft-delete retention 三欄；本表
-- 範圍只限 meeting_files，採同樣的三欄 + 一個通用 metadata。既有
-- transcript_meta (text) 保留不動。
--
-- Per docs/agentk-fusion/fusion-plan.md §2.3 (approved 2026-05-15).

ALTER TABLE meeting_files
    ADD COLUMN IF NOT EXISTS deleted_at         TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS soft_deleted_until TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS hard_delete_after  TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS metadata           JSONB;

-- Sweep worker scans this index to find files that crossed the
-- hard-delete deadline. Partial index keeps it cheap on a mostly-active
-- table.
CREATE INDEX IF NOT EXISTS idx_meeting_files_hard_delete_after
    ON meeting_files(hard_delete_after)
    WHERE hard_delete_after IS NOT NULL;

COMMENT ON COLUMN meeting_files.deleted_at         IS
    'AgentK 對齊：軟刪除時間戳，NULL = active。';
COMMENT ON COLUMN meeting_files.soft_deleted_until IS
    'AgentK 對齊：deleted_at + 30 天；UI 顯示「將於 X 日內可復原」。';
COMMENT ON COLUMN meeting_files.hard_delete_after  IS
    'AgentK 對齊：deleted_at + 60 天；sweep worker 撈出來真刪磁碟+DB。';
COMMENT ON COLUMN meeting_files.metadata           IS
    'AgentK 對齊：通用 JSON metadata。既有 transcript_meta (text) 限定逐字稿元資料，metadata 用於泛用情境。';
