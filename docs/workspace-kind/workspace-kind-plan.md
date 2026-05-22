# 「專案 → 工作區」降級改造計畫

**原則（使用者定，2026-05-19 拍板）**：把「專案」從**強制的頂層組織概念**降級為
工作區的**一種類型 / Todo 的分類**，讓「行政庶務」也能用同一套管，且
**不重寫地基、不碰權限、migration 只增不改不刪**。

---

## 0. 為什麼不是「刪掉專案」

接地調查（2026-05-18）量出的相依面：

| 維度 | 規模 |
|---|---|
| 掛 `project_id` 的資料表 | 12+ 張、13 條 FK |
| `/projects/:id/*` 後端路由 | ~30 條 |
| 權限閘 `user_project_role` / `user_can_access_project` | 跨 11 個後端模組 |
| 接地管線（assemble / build_project_scope / relevant_file_context / firewall 稽核） | 全部以 `project_id` 為鍵 |

`projects` 是 hub table。真刪會動搖地基，且違反兩條既定鐵則：
**(a) migration additive-only；(b) 不碰 ACL（記憶權重政策依賴它）**。

---

## 1. 拍板決議（2026-05-19）

| # | 決定 | 執行原則 |
|---|---|---|
| 1 | `kind ∈ {code, admin, general}` | UI 三類；**後端二元** `is_code = kind=='code'`，admin/general 行為相同。封存另開 `archived_at` 旗標，不佔 kind |
| 2 | 「取消專案」=「降為子標籤、視覺淡化」 | 不刪資料、不改路由，純呈現層 |
| 3 | 行政工作區一樣可掛會議 / AI 彙整 | admin/general 仍走 grounding+firewall，只是無程式碼脈絡（既有 local-project 0-chunk 路徑已驗證） |
| 4 | 既有專案回填 | `Automation_Tools / AgentK_develop / AgentK_FSC = code`；`AI_Agent_Future = general`；其餘測試列（如 `Test`）暫設 `general`（可隨時改） |
| 5 | 評估寫進 docs/ | 本文件 |

---

## 2. 推薦策略：加判別欄、物理不動、語意分流

```
projects.kind  TEXT NOT NULL DEFAULT 'code'
               CHECK (kind IN ('code','admin','general'))
projects.archived_at  TIMESTAMPTZ NULL          -- Phase 4「淡化/封存」用
```

- `code`：現狀全保留（git / clone / 索引 / 接地 / firewall）。
- `admin` / `general`：建立流程**跳過** git/clone/index；Todo / 會議 / 筆記照掛同一 `project_id`。
- **接地天然相容**：非 code 沒 repo → `build_project_scope` 回空 snapshot、
  `relevant_file_context` 回 None → 與既有 local-project 行為一致，grounding **零改動**。
- **ACL 完全不動**：`user_can_access_project` 對任何 kind 適用 → 守住紅線。

---

## 3. 分階段

| 階段 | 內容 | 工時 | 風險 | 狀態 |
|---|---|---|---|---|
| 0 | 文案降級（Projects→工作管理、按鈕→新增工作區，5 語系） | — | 0 | ✅ `cbe9944` |
| 1 | migration 0038：加 `kind` + `archived_at`，回填；`Project` model 帶出 `kind` | 0.5d | 低 | 進行中 |
| 2 | 建立流程依 kind 分流（admin/general 跳過 clone/index）；建立 API 收 `kind` | 1–1.5d | 中 | 待 |
| 3 | 「工作管理」列表依 kind 分組/篩選；行政 kind 隱藏 git/索引 UI；視覺淡化 | 1d | 低 | 待 |
| 4（選） | `archived_at` 封存/收合（非刪除） | 0.5d | 低 | 待 |
| 5（選） | 非 code kind grounding 早退優化（省資源） | 0.5d | 低 | 待 |

每階段完成給「具體、白話的測試方法」。

---

## 4. 不可動紅線

- `user_project_role()` / `user_can_access_project()` 及所有呼叫點 — **整段禁動**（記憶權重政策依賴）。
- 既有資料表欄位 / FK — 只增不改不刪。
- `grounding::assemble` 的 `project_id` 契約 — 不變。

---

## 5. 相依影響摘要

| 層 | 必動 | 自動相容 |
|---|---|---|
| DB | +`kind` +`archived_at`（additive 回填） | 12 子表 FK 全不動 |
| 後端 | 建立/同步流程依 kind 早退；list/get 回 `kind`；建立 API 收 `kind` | 30 條 `/projects/:id/*` 路由語意不變 |
| Grounding | 無（選做早退優化） | assemble/firewall/稽核 全相容 |
| 前端 | 列表分組、建立流程分流、行政隱藏 git UI、視覺淡化 | roadmap/epics/insights/memory 跨「工作區」視圖沿用 |
| 權限/記憶 | 無 | ACL 不變 → 不受影響 |
