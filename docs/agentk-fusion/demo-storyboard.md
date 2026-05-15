# 5/26 Demo Storyboard

**對象**：高層
**標的**：AgentK 會議模組（由 AgentK 主開發主持）
**我們的角色**：5/15 趁主開發休假期間，把 AgentK 會議流程的 schema +
行為先嫁接進 Kway Dev；本 demo **不直接演示 Kway Dev**，但隨時可作為
備援，或在 Q&A 時展示「我們已備齊的對接點」

---

## Demo 主軸（建議講者腳本）

整場圍繞一個故事：**「會議從建立到產出待辦，全自動」**

### 第一幕（2 分鐘）— 建立會議

1. 點「建立會議」→ 全頁表單
2. 填 title / description / start-end / 與會人下拉模糊搜尋（demo 輸入「王」跳出王XX）
3. 地點切「實體」→ 下拉顯示**該時段空閒會議室**（後端 rooms_available endpoint）
4. 按「送出邀請」

**台詞**：「不只本地儲存，也同步到凱衛入口網站 — 不需要再開兩個系統重複登錄」

→ 觀眾看到綠 banner「已同步至凱衛入口網站」

### 第二幕（30 秒）— 凱衛確認

切到凱衛入口網站，conferenceListByWeek.jsp — 那筆會議真的在

### 第三幕（2 分鐘）— Privacy / Busy masking

切到另一位使用者帳號（事先準備好 ws.member 但**非與會者**）
打開 meetings sidebar → 剛建立那場會議顯示為「忙碌（他人預約）」 dashed 灰

**台詞**：「組織內其他人看得到時段被占用，但看不到誰開、為什麼開 — 隱私邊界自動套用」

### 第四幕（3 分鐘）— AI 會議紀錄

1. 回 owner 帳號，會議詳情頁 → Record tab
2. Upload 一份預先準備好的逐字稿 .txt（事先寫好內容包含 1 個決策 + 2 個 action items）
3. 按「產生會議紀錄」
4. 等 ~20 秒 Hermes 回應

**台詞**：「AI 不只整理重點，還區分『已決議』和『待辦事項』 —
後者有負責人對應，可以一鍵變成可追蹤任務」

→ NotesSummary 顯示：
- 摘要（3-5 句）
- 決策（過去式）
- 風險（含 severity）
- **Action Items**（含 assignee 對應到帳號 — 綠色 ●已對應）

### 第五幕（1 分鐘）— Action item → Task sync

點 Action Items 上方「同步成任務」按鈕

→ 顯示「已建立 N 筆任務」
→ 切到該專案 tasks 頁面，看到剛同步過去的任務

**台詞**：「會議結束十秒內，所有未決事項就進入專案管理系統 — 沒有遺漏的『會後再說』」

### 第六幕（1 分鐘）— Realtime + Lifecycle

- 開兩個瀏覽器 tab 看同一場會議
- Tab A 改 status 為「completed」
- Tab B **不 reload 自動**出現 🔒「已鎖定」chip

**台詞**：「會議結束後自動鎖定避免事後竄改紀錄；
有權限的 owner 仍可重新開啟，留下 audit trail」

按「重新開啟」→ 鎖頭消失

### 第七幕（30 秒）— 刪除

刪除一場 demo 用會議 → 凱衛 portal 也同步取消

---

## 備援 / 萬一某段壞掉

| 段落 | 萬一壞了怎麼辦 |
|---|---|
| 凱衛同步 | 改說「目前 selector 在凱衛 portal 更新後需重新校準，但 OAuth 整合是 milestone 2」 |
| AI 產出 | 截圖代替；改說「Hermes Gateway 連線 fluctuation，已備份 dummy response」 |
| Task sync | 改用 SQL 顯示 task_ids JSONB 內容 |
| WS realtime | 手動 F5 也可看到結果 |
| 全部壞掉 | 切到 `docs/agentk-fusion/out/` 看靜態 ER 圖 + lifecycle 圖；改用設計講解 |

---

## Q&A 準備

**Q：為什麼分 Kway Dev / AgentK 兩套？**
A：Kway Dev 是內部開發工具（含凱衛整合等業務專屬功能）；AgentK 是
   產品線。會議模組這邊雙向參考，未來會收斂到一致 API contract。

**Q：權限模型差異？**
A：實際底層相同（4 階）；只是命名習慣不一樣。對照表在 fusion plan 內。

**Q：寄信功能何時做？**
A：deferred to milestone 2，等 SMTP / SendGrid 服務選定後 1 週可上。

**Q：線上會議連結？**
A：UI 已支援手填，自動產生連結等 Webex/Teams OAuth 整合，預計 milestone 2。

**Q：AI 用哪個模型？**
A：透過 Hermes Gateway，模型可切；目前 demo 用 hermes-agent，後端
   `ai_jobs` 表記錄每次呼叫的 provider/model/duration/prompt hash。

**Q：5/26 之後計畫？**
A：（看您決定，我先列）action item assignee 自動 OAuth 通知、
   外部 calendar 兩向 sync、context firewall 對 meeting content 套用。

---

## 預演（5/24-25 兩天）

- 5/24：跑 dry-run.md 全套，找問題
- 5/25：第二輪 dry-run + 講稿排練 + 投影片定稿
- 5/26 上午：環境最終健康檢查 + 截圖備份
- 5/26 demo：照本檔走
