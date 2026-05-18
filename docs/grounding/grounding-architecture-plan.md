# AI Grounding 架構改造計畫

**原則（使用者定）**：後端 AI（Hermes / OpenClaw）執行任何處理，都要能
以**遠端專案**或**本地檔案**為基準、有所根據（evidence-grounded）。

**範圍決策（2026-05-18 拍板）**

| 軸 | 選定 |
|---|---|
| 遠端鮮度 | **兩者都要**：接地前 sync 本地 clone + 可切「即時讀遠端 API」 |
| 檢索 | **字面 + 向量混合** |
| 會議 AI | **接上專案接地 + 過 context firewall** |
| Tool calling | **加**（讓 AI 自己拓檔/查索引） |

---

## 0. 核心設計：統一 Grounding Provider

現在問題的根：接地邏輯**散在聊天流**、會議流是另一條、「遠端」其實塌成本地目錄。
解法是收斂成**一個** module，所有 flow（聊天 / 會議 / 未來其他）+ tool calling
都走它：

```
grounding::assemble(
    project, query, mode, policy,
    source: LocalSynced | RemoteLive,        // 軸1 的切換點
) -> GroundedContext {
    snippets: [{ source, path, git_ref, text }],   // 帶 citation
    blocked:  [...],                                // firewall 擋掉的
    audit:    AgentContextAudit,
}
```

- **單一進入點** → 鮮度、混合檢索、firewall、citation 只實作一次
- 聊天流 / 會議流 / tool calling **共用**，行為一致
- `source` 參數就是使用者要的「以遠端 or 本地為基準」開關

---

## 階段路線（依相依性 + 風險 + 價值排序）

### Phase 0 — 結果（2026-05-18 spike 完成）

| 未知 | 答案 | 對策 |
|---|---|---|
| U1 gateway embeddings | ❌ Hermes 404 / OpenClaw 500，皆不可用 | Phase 3 改本地 `fastembed-rs`（內建 ONNX，零網路） |
| U2 pgvector @ PG18 | ❌ `extension "vector" is not available` | Phase 3 改 `float4[]` 欄 + Rust cosine，不用 pgvector |
| U3 gateway tool calling | ❌ 不支援 client-side tool_calls；Hermes/OpenClaw 是自主 agent，跑自己沙箱的工具，忽略我們的 `tools` | Phase 5 改 ReAct 文字協定（model 輸出 ACTION:，後端攔截執行餵回） |

**最大啟示**：Hermes/OpenClaw 碰不到我們的資料，只能靠後端把內容當文字
預先注入 → Phase 1–4 方向正確且更重要；Phase 3/5 做法調整如上，四軸精神不變。

---

### Phase 0 — 解三個 BLOCKING 未知（spike，0.5–1 天）— 原始描述

動工前必須先確認，否則 Phase 3/5 會做一半卡住：

1. **Embedding provider** — 向量檢索要 embedding。Hermes Gateway 有沒有
   `/v1/embeddings`？或 OpenClaw？或要接 OpenAI-compat 第三方？**未知，要問/試**。
2. **pgvector** — PostgreSQL 18 能不能裝 `vector` extension？（`CREATE EXTENSION vector`）
   裝不了就要外部向量庫或退回純字面。
3. **Gateway tool calling** — 現在 `ChatRequest` 沒有 `tools` 欄、stream 不解析
   tool_calls。`hermes-agent` 透過 gateway 到底支不支援 OpenAI-style function
   calling？**未知，要試**。不支援 → Phase 5 整個要換策略（改 ReAct 文字協定）。

**產出**：一頁 spike 報告，三題各一個明確答案 + 對應的 schema/依賴決定。

### Phase 1 — 抽出統一 Provider（純重構，零行為變更，1 天）

- 把 ws.rs 現有組裝（`build_project_scope` + `relevant_file_context` +
  `secure_agent_context`）抽進新 `backend/src/grounding/mod.rs`
- ws.rs 改呼叫 `grounding::assemble(...)`，**輸出與現在逐位元組相同**
- 加 regression：同一 query 重構前後 context 一致
- **風險最低、是後面所有 phase 的地基**

### Phase 2 — 遠端鮮度（軸 1，2 天）

- **2a 接地前 sync**：git 專案在 `assemble` 前先 fetch + ff（重用
  `git_ops::sync_current_branch`），加 timeout + 失敗 fallback 用舊副本
- **2b 即時讀遠端**：新 `git_ops::remote_contents`（GitHub/GitLab Contents
  API：讀檔案樹 + 單檔內容 by path@ref），rate-limit + 短期快取
- Provider 的 `source` enum 接上：`LocalSynced`（預設、快）/ `RemoteLive`（指定 ref 查特定檔）

### Phase 3 — 混合檢索（軸 2，2–3 天，依賴 Phase 0）

- Migration（additive）：`project_file_chunks` 加 `embedding vector(N)` 欄
- Backfill：重用 `rebuild_project_index` 路徑，逐 chunk 算 embedding
- `relevant_file_context` → 混合：`score = w1·lexical + w2·cosine`，合併去重
- Embedding provider 不可用時**自動退回純字面**（不阻斷）

### Phase 4 — 會議 AI 接地 + firewall（軸 3，1–1.5 天，依賴 Phase 1）

- `generate_ai_notes` 改呼叫 `grounding::assemble`（用該會議的 `project_id`）
- 逐字稿 + 專案 context 一起過 `secure_agent_context`（遮密/分類/稽核）
- 會議紀錄因此會「知道專案程式碼脈絡」，且安全等級與聊天流對齊
- 既有 ai_jobs 稽核 + 補 agent_context_audit_logs

### Phase 5 — Tool calling（軸 4，3–4 天，最大、最後，依賴 Phase 0+1）

- 確認 gateway 支援後：`ChatRequest` 加 `tools`，stream 解析 `tool_calls`
- 定義最小安全工具集，**全部走 Provider + firewall + audit**：
  - `read_file(path, ref)` · `search_index(query)` · `list_tree(path)`
- Orchestrator 加 tool-loop：model → 有 tool_calls 就執行 → 餵回 → 重跑，
  設 max iterations 防無限迴圈
- 工具輸出**一樣過 context_firewall**（不能繞過遮密）
- 風險最高：streaming + tool loop + firewall 三者交互

---

## 跨階段：Evidence / Citation 契約

`GroundedContext.snippets` 每筆帶 `{source, path, git_ref}`。回應層加一個
citation 規範，讓 AI 產出能標「依據 <檔案>@<ref>」。會議紀錄 / 聊天 / tool
輸出統一格式 → 真正「有所根據」可追溯。

---

## 總工時與排序建議

| Phase | 工時 | 風險 | 阻塞 |
|---|---|---|---|
| 0 spike | 0.5–1d | – | 解三個未知 |
| 1 Provider 抽取 | 1d | 低 | 無 |
| 2 遠端鮮度 | 2d | 中 | 無 |
| 4 會議接地+firewall | 1–1.5d | 低 | Phase 1 |
| 3 混合檢索 | 2–3d | 中 | Phase 0 (embedding) |
| 5 tool calling | 3–4d | 高 | Phase 0 (gateway) + Phase 1 |

**合計約 10–13 工作天。**

建議順序：**0 → 1 → 4 → 2 → 3 → 5**
理由：
- 0 先解未知（不然 3/5 會做廢）
- 1 是地基且零風險
- 4 緊接 1，立刻讓會議流（剛做完的模組）拿到接地價值，CP 值最高
- 2 不依賴未知，穩做
- 3 等 0 的 embedding 答案
- 5 最後，風險最高，且依賴 0 的 gateway 答案

---

## 與 5/26 AgentK demo 的關係

這條 10–13 天，跟 5/26 demo 衝突。建議：
- **Demo 前**只做 Phase 0（spike）+ Phase 1（重構，零風險）+ Phase 4
  （會議接地，demo 看得到價值）
- Phase 2/3/5 排 demo 後
- Phase 0 spike 結果可能改變 3/5 的做法，越早做越好

---

## 待你拍板

1. 同意「統一 Grounding Provider」這個收斂方向？（後面全部建在它上面）
2. 同意排序 `0 → 1 → 4 → 2 → 3 → 5`？
3. Demo 前範圍是否就鎖 Phase 0 + 1 + 4？
4. Phase 0 spike 我現在就開跑嗎？（要動到查 Hermes/OpenClaw gateway 能力、試 pgvector）
