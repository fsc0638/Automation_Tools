# AgentK 會議功能融合計畫（v1）

**Author:** 2026-05-15
**Scope:** 只動會議模組。Kway Dev 是主體，AgentK 是補強來源。
**Schema rule:** **只新增、不更新、不刪除**。現有與 AgentK 用途相似的欄位**全部保留**，新欄位只補真正缺的能力。
**權限 rule:** **不動**。Kway Dev 既有 owner/admin/editor/viewer 的記憶權重機制原樣保留。

---

## 1. 交叉比對總覽

✅ 已有 / ⚠️ 部分（命名差或行為差）/ ❌ 缺

### 1.1 Domain & schema

| 概念 | AgentK | Kway Dev 現況 | 狀態 | 結論 |
|---|---|---|---|---|
| 會議主表 | `meetings` | `meetings` | ✅ | 用我們的 |
| Tenant 範圍 | `workspace_id` | `organization_id` | ✅ | 用我們的 |
| 專案歸屬 | （無，靠 workspace） | `project_id` | ✅ | 用我們的 |
| 標題 | `title` | `title` | ✅ | 用我們的 |
| 長描述 | `description` | `notification_note`（語意是「通知說明」非「會議介紹」） | ⚠️ | **+ `description`** 新欄位，與既有並存 |
| 狀態 | `status` 4 值 | `status` 5 值（多 `in_progress` `completed`） | ⚠️ | 用我們的；AgentK 的 `ended` 對應我們的 `completed` |
| 鎖（脫離狀態） | `is_locked` bool | （無；狀態暗含） | ❌ | **+ `is_locked BOOLEAN NOT NULL DEFAULT false`** |
| 開始時間 | `starts_at` | `start_at` | ✅ | 用我們的（只差複數 s） |
| 結束時間 | `ends_at` | `end_at` | ✅ | 用我們的 |
| 全日活動 | （無） | `all_day` | ✅ | 我們多了這個 |
| 重複 | （無） | `recurrence` | ✅ | 我們多了這個 |
| 時區 | （無） | `timezone` | ✅ | 我們多了這個 |
| 實體地點 | `location` | `location` | ✅ | 用我們的 |
| 線上連結 | `join_url` | （無；線上 provider 只有前端顯示） | ❌ | **+ `join_url TEXT`** |
| 重要程度 | （無） | `importance` | ✅ | 我們多了這個 |
| 邀請寄出時間 | （無） | `invitations_sent_at` | ✅ | 我們多了這個 |
| 建立者 | `created_by_user_id` | `creator_id` | ✅ | 用我們的 |
| 最後修改者 | `updated_by_user_id` | （無） | ❌ | **+ `updated_by_user_id UUID NULL REFERENCES users(id)`** |
| 外部 provider | `external_provider`（symbolic：webex/teams/meet） | `external_id`（opaque 字串：`portal:CODE-DATE-HHMM`） | ⚠️ | 用途不同，兩個都保留；**+ `external_provider TEXT`** |
| 外部事件 ID | `external_event_id` | （無） | ❌ | **+ `external_event_id TEXT`** |
| 外部事件連結 | `external_event_url` | （無） | ❌ | **+ `external_event_url TEXT`** |
| 同步狀態 | `sync_status` | `portal_booked_at`+`portal_book_error`（凱衛專用） | ⚠️ | 兩個保留；**+ `sync_status TEXT`** 給通用 |
| 最後同步時間 | `last_synced_at` | `portal_booked_at`（半重疊） | ⚠️ | 兩個保留；**+ `last_synced_at TIMESTAMPTZ`** |
| 凱衛 portal 預約 | （無） | `portal_booked_at`/`portal_book_error`/`external_creator_name` | ✅ | 我們獨有，保留 |
| 專屬聊天室 | `room_id` 1:1 | （無 room 概念） | ❌ | **暫不加**（需要先有 room/conversation 對接設計，5/26 前不做） |
| 與會人表 | `meeting_participants` | `meeting_attendees` | ✅ | 用我們的 |
| 與會人角色 | `role`：owner/delegate/attendee | `role_label`（free text） | ⚠️ | **不加欄位**；只在程式碼把值標準化為 `admin`/`editor`/`viewer`（既有 role_label TEXT 直接吃） |
| 出席確認 | （無） | `confirmation_status`/`confirmed_at`/`dispute_note` | ✅ | 我們多了這個 |
| 與會人 `updated_at` | 有 | 無 | ❌ | 低優先，可不加 |

### 1.2 會議紀錄

| 概念 | AgentK | Kway Dev 現況 | 狀態 | 結論 |
|---|---|---|---|---|
| 紀錄主表 | `meeting_records`（1:1 per meeting） | `meeting_notes`（多版本） | ⚠️ | 用我們的；多版本是優勢 |
| 摘要 | `summary` | `summary` | ✅ | |
| 決議 | `decisions_json` | `decisions` jsonb | ✅ | |
| 風險 | （無） | `risks` jsonb | ✅ | 我們多了這個 |
| 逐字稿引用 | `transcript_ids_json`（refs） | `transcript_excerpts` jsonb（內容） | ⚠️ | 兩種模式不同；保留 excerpts，未來再加 transcript table |
| **Action items** | `action_items_json`：`{title, description, assignee_user_id, source}[]` | （無；只有 `meeting_task_impacts` 表記任務影響） | ❌ | **+ `action_items JSONB NOT NULL DEFAULT '[]'`** 到 `meeting_notes` |
| AI 任務追溯 | `ai_job_ids_json` | （無） | ❌ | **+ `ai_job_ids JSONB NOT NULL DEFAULT '[]'`** |
| 同步出去的 task | `task_ids_json` | `meeting_task_impacts.task_id` | ⚠️ | 兩種模式不同；額外加 **+ `task_ids JSONB NOT NULL DEFAULT '[]'`** 給快速查找 |
| 修改者 | `updated_by_user_id` | `meeting_notes_edits.edited_by`（在 edits 表） | ✅ | 用我們的 audit table |
| 紀錄編輯 audit | （無） | `meeting_notes_edits` | ✅ | 我們多了這個 |
| 任務影響（progress） | （無） | `meeting_task_impacts.progress_from/to` | ✅ | 我們多了這個 |

### 1.3 檔案 / Assets

| 概念 | AgentK | Kway Dev 現況 | 狀態 | 結論 |
|---|---|---|---|---|
| 檔案 metadata | `assets`（通用 layer） | `meeting_files`（meeting 專屬） | ⚠️ | 用我們的；不引入通用層（會牽動 audio 等） |
| 儲存路徑 | `storage_provider`+`storage_key` | `storage_path` | ✅ | 用我們的 |
| 分類 | `asset_kind` | `file_category` | ✅ | 用我們的 |
| 上傳狀態 | `status` (active/soft_deleted/hard_deleted) | `upload_status`（pending/done/failed） | ⚠️ | 用途不同（lifecycle vs 上傳結果）；都保留 |
| **軟刪除戳記** | `deleted_at` | （無；目前直接刪） | ❌ | **+ `deleted_at TIMESTAMPTZ NULL`** |
| **軟刪寬限到期** | `soft_deleted_until` | （無） | ❌ | **+ `soft_deleted_until TIMESTAMPTZ NULL`** |
| **硬刪截止** | `hard_delete_after` | （無） | ❌ | **+ `hard_delete_after TIMESTAMPTZ NULL`** |
| 通用 metadata | `metadata_json` | `transcript_meta` (text) | ⚠️ | 用途相似；保留我們的，補一個泛用：**+ `metadata JSONB NULL`** |
| 通用 size_bytes | `size_bytes` | `file_size` | ✅ | 用我們的 |

### 1.4 行為層（不涉 schema）

| 概念 | AgentK | Kway Dev 現況 | 狀態 |
|---|---|---|---|
| Busy masking（非與會者只看占用） | 有，calendar/list 都套用 | ❌ 缺 |
| Realtime 事件（meeting.created/scheduled/ended/...） | 8 個 WS event | ❌ 缺 |
| 會議鎖定阻擋 room 寫入 | 有 | ❌ 我們沒 room |
| Action item → task 自動同步（去重） | 有 | ⚠️ 有 task_impacts 但是手動 |
| AI 產 minutes via ai_jobs adapter | 有 | ⚠️ generate endpoint 是 stub |
| 檔案 soft-delete retention worker | stub | ❌ 缺 |
| 外部 provider OAuth | 預留欄位、不接 | ❌ 缺（我們用 portal scrape 替代） |

---

## 2. DB 新增欄位清單（additive only）

### 2.1 Migration `0032_meetings_agentk_alignment.sql`

```sql
-- meetings: 新增 AgentK 對齊欄位（不動既有）
ALTER TABLE meetings
    ADD COLUMN IF NOT EXISTS description           TEXT,
    ADD COLUMN IF NOT EXISTS is_locked             BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS join_url              TEXT,
    ADD COLUMN IF NOT EXISTS external_provider     TEXT,
    ADD COLUMN IF NOT EXISTS external_event_id     TEXT,
    ADD COLUMN IF NOT EXISTS external_event_url    TEXT,
    ADD COLUMN IF NOT EXISTS sync_status           TEXT,
    ADD COLUMN IF NOT EXISTS last_synced_at        TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS updated_by_user_id    UUID REFERENCES users(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_meetings_is_locked ON meetings(is_locked) WHERE is_locked = TRUE;

COMMENT ON COLUMN meetings.description       IS 'AgentK 對齊：會議長描述。與 notification_note 不同——後者是寄給與會人的通知文字，前者是會議本身的介紹。';
COMMENT ON COLUMN meetings.is_locked         IS 'AgentK 對齊：獨立於 status 的鎖。status=completed 時應自動設為 true；admin/owner 可清除（reopen）。';
COMMENT ON COLUMN meetings.join_url          IS 'AgentK 對齊：線上會議直接連結（Webex / Teams / Meet）。';
COMMENT ON COLUMN meetings.external_provider IS 'AgentK 對齊：外部 provider symbol（webex/teams/meet/kway-portal）。與既有 external_id（opaque 字串）並存。';
COMMENT ON COLUMN meetings.sync_status       IS 'AgentK 對齊：通用同步狀態。與既有 portal_booked_at/portal_book_error（凱衛專用）並存。';
COMMENT ON COLUMN meetings.updated_by_user_id IS 'AgentK 對齊：最後修改者。既有 updated_at 已有時間，這裡補審計人。';
```

### 2.2 Migration `0033_meeting_notes_records_alignment.sql`

```sql
-- meeting_notes: 補 records aggregate 三欄
ALTER TABLE meeting_notes
    ADD COLUMN IF NOT EXISTS action_items  JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS ai_job_ids    JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS task_ids      JSONB NOT NULL DEFAULT '[]'::jsonb;

COMMENT ON COLUMN meeting_notes.action_items IS
    'AgentK 對齊：{title, description, assignee_user_id, source}[]。與 risks/decisions 同層級的列表。';
COMMENT ON COLUMN meeting_notes.ai_job_ids   IS
    'AgentK 對齊：產生此 note 的 AI job ID 列表（未來接 ai_gateway 時用，目前可空）。';
COMMENT ON COLUMN meeting_notes.task_ids     IS
    'AgentK 對齊：action_items 同步出去的 project_tasks ID 快取。實際 mutation 仍走 meeting_task_impacts。';
```

### 2.3 Migration `0034_meeting_files_retention.sql`

```sql
-- meeting_files: 補軟刪除 retention 三欄
ALTER TABLE meeting_files
    ADD COLUMN IF NOT EXISTS deleted_at         TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS soft_deleted_until TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS hard_delete_after  TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS metadata           JSONB;

CREATE INDEX IF NOT EXISTS idx_meeting_files_hard_delete_after
    ON meeting_files(hard_delete_after)
    WHERE hard_delete_after IS NOT NULL;

COMMENT ON COLUMN meeting_files.deleted_at         IS 'AgentK 對齊：軟刪除時間戳，NULL=active。';
COMMENT ON COLUMN meeting_files.soft_deleted_until IS 'AgentK 對齊：deleted_at + 30 天；UI 顯示「將於 X 日內可復原」。';
COMMENT ON COLUMN meeting_files.hard_delete_after  IS 'AgentK 對齊：deleted_at + 60 天；sweep worker 撈出來真刪磁碟+DB。';
COMMENT ON COLUMN meeting_files.metadata          IS 'AgentK 對齊：通用 JSON metadata（既有 transcript_meta 是 text 限定逐字稿用）。';
```

### 2.4 不增加的部分

| AgentK 概念 | 不加的理由 |
|---|---|
| `meetings.room_id` + dedicated room | 我們沒 room/conversation 對接設計，加了空欄位無意義 |
| `meeting_participants.updated_at` | 純審計，5/26 前用不到 |
| 通用 `assets` table | 影響範圍超出 meeting（會牽動 audio_transcripts 等），不在本期 scope |

---

## 3. Backend 程式碼變更（不涉 schema 的部分）

### 3.1 必做（5/26 demo 前）

| 變更 | 檔案 | 工時 |
|---|---|---|
| **busy masking**：`list_meetings` 對非 attendee/creator 的 user 把 `title`/`location`/`notification_note` 換成 NULL，加 `visibility: 'full'\|'busy'` | `backend/src/api/meetings.rs` | 0.5 d |
| `MeetingDetail` schema 補 `description`/`is_locked`/`join_url`/`updated_by_user_id`/`action_items`/`ai_job_ids`/`task_ids` 序列化 | 同上 | 0.5 d |
| `create_meeting` / `update_meeting` 支援新欄位寫入 | 同上 | 0.5 d |
| `status='completed'` 時自動 set `is_locked=TRUE`，reopen endpoint `POST /meetings/:id/reopen` 清除 is_locked（admin / owner only） | 同上 | 0.5 d |
| `meeting_files` 軟刪改成 stamp `deleted_at`/`soft_deleted_until`/`hard_delete_after` 而非實際 unlink | `meetings.rs::delete_file` | 0.5 d |
| Background sweep：`tokio` task 每日掃 `hard_delete_after < NOW()` 真刪磁碟 + DB | `main.rs` + 新檔 `file_retention.rs` | 1 d |

### 3.2 次優先（demo 後）

- Realtime 事件（meeting.created/scheduled/ended/cancelled/lock_changed） → 需要 WS pub/sub 基礎建設（已在 deferred_backlog #3）
- Action item → task 自動同步：`POST /meetings/:id/notes/sync-tasks`（接 `project_tasks`，去重）
- AI minutes generation：接 `agents/hermes` 或 OpenClaw 真實產 record（目前 `generate_ai_notes` 是 stub）

---

## 4. Frontend 變更

### 4.1 必做（5/26 demo 前）

| 變更 | 檔案 |
|---|---|
| 新會議表單加 `description` textarea（與既有 `notification_note` 並列） | `web/src/app/(app)/meetings/new/page.tsx` |
| 線上會議 toggle 真寫入 `join_url`（既有只存到 location） | 同上 |
| 詳情頁顯示 `is_locked` 鎖頭 icon + reopen 按鈕（admin/owner only） | `web/src/app/(app)/meetings/[id]/page.tsx` |
| Calendar / list 卡片區分 `visibility: 'busy'` 樣式（灰底、無標題） | `web/src/components/meetings/MeetingSidebar.tsx`、`MeetingCalendarGrid.tsx` |
| 詳情頁 Records 區塊：summary / decisions / risks / action_items（含 assignee 下拉） | `web/src/components/meetings/NotesSummary.tsx` 等 |
| 檔案列表標示 soft-deleted 狀態 + 剩餘可復原天數 | `web/src/components/meetings/FileWorkspace.tsx` |

### 4.2 次優先

- 「Sync action items 成任務」按鈕（依賴 backend #3.2）
- Workbench 主舞台對齊 AgentK.pen（如果還有時間）

---

## 5. 5/26 Demo 優先序（建議 11 天工時分配）

| 日期 | 主題 | 對應上面段落 |
|---|---|---|
| 5/15 (五) | **修 portal 預約 selector**（昨晚 e2e 失敗） + 寫這份計畫定案 | – |
| 5/18 (一) | Migration 0032/0033/0034 + 後端 `description`/`is_locked`/`join_url` 寫入 | §2 + §3.1 前三項 |
| 5/19 (二) | `is_locked` 自動規則 + reopen endpoint + 前端鎖頭 UI | §3.1 第 4 項 + §4.1 第 3 項 |
| 5/20 (三) | **Busy masking**（後端 + 前端） | §3.1 第 1 項 + §4.1 第 4 項 |
| 5/21 (四) | Action items（action_items / ai_job_ids / task_ids 欄位 + UI） | §3.1 + §4.1 第 5 項 |
| 5/22 (五) | 檔案 soft-delete + sweep worker + UI | §3.1 後兩項 + §4.1 第 6 項 |
| 5/23 (六) | Buffer / 修 bug | – |
| 5/24 (日) | E2E demo 預演 + 抓出來不順的點 | – |
| 5/25 (一) | Demo 細節（投影片 / 流程稿 / 切換動線） | – |
| 5/26 (二) | **Demo 日** | – |

**不在這條時間軸上的**：dedicated room / realtime / action item 自動 sync to task / AI minutes 真實接 ai_gateway — 都等 5/18 主開發回來再排。

---

## 6. 風險 / 未決定

1. **`completed` → `is_locked` 自動化**：既有 `completed` 列要不要回頭設 `is_locked=TRUE`？migration 預設 false，新規則只對 `completed` 之後的列觸發。**建議**：補一句 backfill：
   ```sql
   UPDATE meetings SET is_locked = TRUE WHERE status = 'completed';
   ```
   會動既有資料，但符合語意。需要您點頭。

2. **AgentK 主開發 5/18 回來後**：他預期看到什麼？我這邊做的這幾步，是要 PR 給他？還是讓他直接用我們的版本介接？這影響 commit message 風格 + 是否要寫對應的 dev-report。**待您回**。

3. **Demo 那天跑的是 AgentK 還是我們的系統？** — 影響 #4 前端要不要做 AgentK.pen 視覺對齊。**待您回**。

4. **portal 預約 selector 還沒驗證**：昨晚 e2e 失敗、selector 是猜的。這條若不修，demo 時「會議建立後同步至凱衛」這個 selling point 會塌。**建議今天 5/15 第一件事先修這個**。

---

## 7. 核可結果（2026-05-15）

- [x] §2 三個 migration 全部照列表加，相似用途欄位**全部並存**
- [x] §5 優先序照計畫
- [x] §6.1 backfill `completed → is_locked=TRUE` — **執行**；migration 0032 末尾加上：
  ```sql
  UPDATE meetings SET is_locked = TRUE WHERE status = 'completed';
  ```
- [x] §6.2 demo 那天跑 **AgentK**（不是我們的部署）
- [x] §6.3 **不做 AgentK.pen 視覺對齊** — 我們這邊不花時間在 UI 像素對齊，重點放在 schema + 行為層

### 因此調整的範圍

§4 Frontend 變更段降級成「**夠用就好**」：
- 仍補 `description` / `join_url` / `is_locked` / busy masking / action items / soft-delete 提示等**最小可操作 UI**
- **不重做**版面、卡片、推薦時段等視覺細節
- 重心放在 schema + backend，讓 5/18 AgentK 主開發回來時可以對齊或抽取出去
