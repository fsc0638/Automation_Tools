# Demo Dry-Run Checklist

執行時對著本檔逐項打勾，失敗時記下 timestamp + DB 狀態 + 截圖路徑。

**Pre-check**
- [ ] Backend healthz → 200：`curl http://localhost:8888/healthz`
- [ ] Frontend 起得來：`http://localhost:3000`
- [ ] 凱衛 portal 可登入（手動開 https://crm.kway.com.tw 確認）
- [ ] HERMES_API_URL 可達（避免 generate 失敗）

---

## Scenario 1：完整建會 → portal → AI minutes → task sync → 鎖定 → reopen → 刪除

| 步驟 | 動作 | 預期 |
|---|---|---|
| 1 | 開 `/meetings/new`，填明天 14:00–15:00、地點選實體某會議室、與會人選 2 人、按「送出邀請」 | 跳轉到詳情頁、綠 banner「已同步至凱衛入口網站」 |
| 2 | 開凱衛 portal `conferenceListByWeek.jsp` 確認該室該時段顯示為「范書愷」 | ✅ 預約存在 |
| 3 | 回 detail 頁切到 **Record** tab → 點 Upload，上傳一個 .txt 假逐字稿 | 檔案出現在列表 |
| 4 | 按「產生會議紀錄」 | 約 15–30s 後 NotesSummary 顯示摘要 / 決策 / 風險 / **Action Items** / 逐字稿引用 |
| 5 | Action Items 區塊點「同步成任務」 | 顯示「已建立 N 筆任務」（前提：meeting 有 project_id；沒有的話顯示「未連結至專案」） |
| 6 | 開該 project 的 tasks 頁面確認新任務存在、title 一致 | ✅ task 出現 |
| 7 | 回 detail 頁，編輯會議將 status 改 `completed`（或在 list 改） | 標題列出現 🔒 「已鎖定」 chip + 「重新開啟」按鈕 |
| 8 | 按「重新開啟」 | chip 消失、按鈕消失（再變回可編輯狀態） |
| 9 | 標題列垃圾桶刪除會議 | 跳轉回 `/meetings`，凱衛 portal 那個時段也清掉 |

**WS realtime 同步驗證**：開兩個 browser tab 看同一場會議，在 tab A 改狀態，tab B 應該自動 refetch（不用 reload）。

---

## Scenario 2：busy masking

| 步驟 | 動作 | 預期 |
|---|---|---|
| 1 | 用 user X 建一場明天的會議（不要邀請 user Y） | 建好 |
| 2 | 用 user Y 登入，看 sidebar「近期會議」 | X 那場顯示為 dashed 灰色 + 「忙碌（他人預約）」+ 標題與會人不可見 |
| 3 | user Y 點該 row | 不可點（前端 disabled） |

---

## Scenario 3：自動 lock + 編輯 → 同步行為

| 步驟 | 動作 | 預期 |
|---|---|---|
| 1 | 建一場會議 status='scheduled' | 一切正常 |
| 2 | PATCH `/meetings/{id}` body `{"status":"completed"}` | DB: is_locked → TRUE（auto rule）|
| 3 | UI reload 後看到 🔒 chip | ✅ |
| 4 | 再 PATCH 想改 title 等欄位（不送 is_locked / status） | 應成功（we 不強擋；只是 status 仍 completed） |

---

## Scenario 4：portal 重複預約 → 失敗保留 draft

| 步驟 | 動作 | 預期 |
|---|---|---|
| 1 | 建會議 A 在 5號會議室 明天 10:00–11:00、送出邀請 | 綠 banner 預約成功 |
| 2 | 建會議 B 在 5號會議室 同時段、送出邀請 | 紅 banner「portal reported: 時間錯誤」或「已被預約」，status='draft'，可改地點後再送 |

---

## Scenario 5：portal cancel — 包含 scrape 進來的

| 步驟 | 動作 | 預期 |
|---|---|---|
| 1 | sidebar 中找一個 scrape 進來的會議（external_id IS NOT NULL）→ 不是 busy 的（如果是 busy 換用 SQL 直接 DELETE 測 ws event） | 找到 |
| 2 | 進詳情頁刪除 | 凱衛 portal best-effort 嘗試取消（可能因為 1665 不是該預約人而被擋） |
| 3 | 看後端 log | 看到「portal cancel failed for meeting X」warn line，但本地 DB 已刪 |

---

## DB 健康檢查（每輪後跑）

```sql
-- 應該沒有孤兒 portal_book_error 紀錄
SELECT COUNT(*) FROM meetings WHERE portal_book_error IS NOT NULL;

-- ai_jobs 應該每場 generate 都對應一筆
SELECT kind, status, COUNT(*) FROM ai_jobs GROUP BY 1, 2;

-- task_ids 與 project_tasks 對齊
SELECT m.id, jsonb_array_length(mn.task_ids) AS task_count
FROM meetings m JOIN meeting_notes mn ON mn.meeting_id = m.id
WHERE jsonb_array_length(mn.task_ids) > 0;

-- soft-deleted files 數量
SELECT COUNT(*) FROM meeting_files WHERE deleted_at IS NOT NULL;
```

---

## Demo 當天前 24 小時

- [ ] 重跑 Scenario 1 一輪 — 全綠
- [ ] 清掉測試會議避免 portal 占位干擾 demo
- [ ] 確認 HERMES_API_KEY 還沒過期
- [ ] 截圖每個 banner / chip / 鎖頭 icon 留檔備援（萬一現場跑掛了用截圖代替）
