# Hermes Meeting Learning Loop × 歷史感知 ToDo 維護

## 結論

不要做成單次「會議逐字稿抽 ToDo」。
要做成：

```text
Transcript / AudioTranscript
→ 偵測專案提及
→ 專案別名 / 簡稱解析
→ 對應 canonical meeting project
→ 載入該專案既有 Task / Task history / Project memory
→ Hermes 產生 ToDo change proposals
→ 寫入 LearningCandidate
→ 人工審核
→ 套用正式 Task / TaskEvent / Alias / Memory
```

核心要求：

- 同一專案不同叫法要能解析成同一 canonical project。
- 新會議不能每次只新增 ToDo。
- 必須依歷史狀態產生：
  - `create`
  - `update`
  - `complete`
  - `reopen`
  - `supersede`
  - `no_change`
  - `needs_clarification`
- AI 只能提案，不可直接永久改正式資料。
- 每個 ToDo proposal 必須有 transcript citation。

---

## 1. DB / Model 修改

### 修改檔案

```text
product/services/backend/app/db/models.py
product/services/backend/app/schemas/chat.py
product/services/backend/alembic/versions/<new_revision>_meeting_learning_loop.py
```

也可新增：

```text
product/services/backend/app/schemas/meeting_learning.py
```

### 新增 `meeting_projects`

Canonical 專案實體。

```python
id
workspace_id
canonical_name
description
status  # active / archived
created_at
updated_at
```

### 新增 `meeting_project_aliases`

```python
id
workspace_id
meeting_project_id
alias
normalized_alias
source_type  # manual / ai_candidate / transcript_observed
confidence
status       # pending / approved / rejected
created_at
reviewed_at
```

規則：

- `approved` 前不可作為正式解析依據。
- AI 只能提出 alias candidate。
- 人審後才生效。

### 新增 `meeting_project_mentions`

```python
id
workspace_id
audio_transcript_id
meeting_project_id nullable
raw_mention
normalized_mention
start_offset
end_offset
confidence
resolution_status  # resolved / ambiguous / unresolved
evidence_json
created_at
```

用途：保存「為什麼這段逐字稿被判斷成某專案」的證據。

### 擴充既有 `Task`

不要另開第二套 task system，直接擴充現有 `Task`：

```python
meeting_project_id nullable
source_audio_transcript_id nullable
source_ai_job_id nullable
source_citation_json nullable
last_seen_at nullable
confidence nullable
```

### 新增 `task_events`

```python
id
task_id
event_type  # created / updated / completed / reopened / superseded / clarified
source_ai_job_id
source_audio_transcript_id
before_json
after_json
rationale
created_at
```

這是「專案歷史進程」的核心表。

### 新增 `learning_candidates`

```python
id
workspace_id
source_ai_job_id
candidate_type  # project_alias / todo_update / project_memory / extraction_rule
payload_json
confidence
status          # pending / approved / rejected / applied
review_note
created_at
reviewed_at
applied_at
```

---

## 2. Backend 核心服務

新增檔案：

```text
product/services/backend/app/core/project_identity.py
product/services/backend/app/core/todo_reconciliation.py
product/services/backend/app/core/meeting_learning.py
```

### `project_identity.py`

負責專案名稱標準化與解析。

```python
def normalize_project_name(name: str) -> str:
    ...
```

需處理：

- 大小寫
- 空白
- dash / underscore
- 中英文括號
- 常見尾詞：`專案`、`project`、`系統`、`平台`

```python
def resolve_project_mention(
    db,
    workspace_id,
    raw_mention: str,
    context_snippet: str,
) -> ProjectResolution:
    ...
```

判斷順序：

1. approved alias exact match
2. normalized alias match
3. canonical name fuzzy match
4. workspace 近期高頻專案輔助
5. Hermes semantic suggestion
6. 信心不足 → `ambiguous`

門檻：

```text
>= 0.90       resolved
0.65 - 0.89  ambiguous，需要人審
< 0.65       unresolved
```

### `todo_reconciliation.py`

把抽取結果轉成「變更提案」，不是 task list。

```python
def reconcile_extracted_todos(
    existing_tasks: list[Task],
    extracted_items: list[ExtractedTodo],
    project_history: dict,
) -> list[TodoChangeProposal]:
    ...
```

允許輸出：

```text
create
update
complete
reopen
supersede
no_change
needs_clarification
```

例：

> K專案 alias resolution migration 已完成，下週補 review UI。

應產生：

```text
complete：alias resolution migration
create：review UI
```

不是新增兩個重複 ToDo。

### `meeting_learning.py`

```python
def run_meeting_learning_job(
    db,
    ai_job_id,
    audio_transcript_id,
) -> MeetingLearningResult:
    ...
```

流程：

1. 載入 `AudioTranscript`
2. 切 transcript segments，保留 offset
3. 偵測 project mentions
4. resolve canonical project
5. 載入 approved aliases、existing tasks、task_events、project memory
6. 呼叫 Hermes structured extraction
7. 執行 todo reconciliation
8. 寫入 `LearningCandidate`
9. 等待人工審核
10. approve 後才 apply

---

## 3. AIJob 整合

修改：

```text
product/services/backend/app/core/ai_jobs.py
product/services/backend/app/core/chat.py
product/services/backend/app/core/ai_gateway.py
```

沿用既有 `AIJob` lifecycle，不另開 queue。

新增 job type / capability：

```text
meeting_todo_extraction
project_alias_resolution
todo_reconciliation
```

`context_snapshot` 建議包含：

```json
{
  "audio_transcript": "...",
  "detected_project_mentions": [],
  "approved_aliases": [],
  "candidate_projects": [],
  "existing_project_todos": [],
  "recent_task_events": [],
  "project_history_summary": "..."
}
```

---

## 4. Hermes Structured Output Schema

修改或新增：

```text
product/packages/contracts/json-schema/task-extraction.schema.json
```

不要輸出：

```json
{"tasks": ["A", "B", "C"]}
```

要輸出 change proposals：

```json
{
  "project_mentions": [
    {
      "raw_name": "K專案",
      "resolved_project_id": null,
      "confidence": 0.72,
      "evidence_quote": "K專案這週要把逐字稿轉 ToDo 做完"
    }
  ],
  "todo_changes": [
    {
      "action": "create | update | complete | reopen | supersede | no_change | needs_clarification",
      "existing_task_id": null,
      "project_raw_name": "K專案",
      "project_resolution_confidence": 0.72,
      "title": "...",
      "description": "...",
      "status": "todo | in_progress | done",
      "rationale": "...",
      "citation": {
        "quote": "...",
        "start_offset": 123,
        "end_offset": 180
      },
      "confidence": 0.81
    }
  ],
  "learning_candidates": [
    {
      "candidate_type": "project_alias",
      "payload": {},
      "confidence": 0.78,
      "reason": "..."
    }
  ]
}
```

硬規則：

- 每個 `todo_change` 必須有 citation。
- ambiguous project 不可自動 apply。
- `update / complete / reopen` 優先於 `create`。
- `no_change` 是有效輸出，代表已比對歷史。

---

## 5. API 修改

修改：

```text
product/services/backend/app/api/workspaces.py
product/packages/contracts/openapi/backend.yaml
```

新增 endpoints：

```http
POST /workspaces/{workspace_id}/rooms/{room_id}/transcripts/{transcript_id}/meeting-learning-jobs
```

建立 meeting learning AIJob。

```http
GET /workspaces/{workspace_id}/learning-candidates
```

支援 filter：

```text
status=pending
candidate_type=project_alias
candidate_type=todo_update
```

```http
POST /workspaces/{workspace_id}/learning-candidates/{candidate_id}/approve
POST /workspaces/{workspace_id}/learning-candidates/{candidate_id}/reject
```

審核候選項目。

```http
GET /workspaces/{workspace_id}/meeting-projects
GET /workspaces/{workspace_id}/meeting-projects/{project_id}/timeline
```

查看 canonical projects 與歷史進程。

---

## 6. Frontend Review UI

修改：

```text
product/apps/frontend/src/app/page.tsx
product/apps/frontend/src/lib/chat-client.ts
```

第一版先放在既有 transcript / AIJobsPanel 附近。

UI 流程：

1. 選 transcript
2. 點「分析會議 ToDo」
3. 顯示 AIJob 狀態：
   - `queued`
   - `running`
   - `completed`
   - `failed`
4. completed 後顯示三區：

### A. Project mentions

顯示：

- 原文名稱
- 對應 canonical project
- confidence
- resolved / ambiguous / unresolved
- transcript quote

ambiguous 時允許手動指定 project。

### B. Todo change proposals

顯示：

- action
- 舊 task 狀態
- 建議新狀態
- rationale
- citation quote
- approve / reject

### C. Learning candidates

顯示：

- alias candidate
- project memory candidate
- extraction rule candidate
- approve / reject

UI 重點：  
**AI 建議如何更新專案既有任務狀態，而不是抽一批新任務。**

---

## 7. Apply 規則

可以自動保存：

```text
AI raw output
structured result
detected mentions
citations
pending learning candidates
```

不可自動永久套用，除非使用者 approve：

```text
新增正式 alias
合併 project
關閉 task
修改 task 狀態
更新 project memory
調整 extraction rule
```

approve 後才執行：

```text
project_alias   → 新增 approved alias
todo_update     → 寫入 Task + TaskEvent
project_memory  → 更新 project memory summary
extraction_rule → 保留給後續 prompt / eval 使用
```

---

## 8. 測試

新增測試檔：

```text
product/services/backend/tests/test_project_identity_resolution.py
product/services/backend/tests/test_meeting_todo_reconciliation.py
product/services/backend/tests/test_learning_candidates.py
product/services/backend/tests/test_meeting_learning_api.py
```

必測：

1. `AgentK`、`K專案`、`Kway automation` 在 alias approved 後解析為同一 project
2. 未 approved alias 不可自動解析
3. ambiguous mention 會產生 review candidate
4. 同一 ToDo 再次出現不會重複 create
5. 提到「已完成」產生 `complete`
6. 提到「延到下週」產生 `update`
7. 提到「重新打開」產生 `reopen`
8. 無 citation 的 proposal 不可 apply
9. reject candidate 不會寫入正式資料
10. approve alias 後，後續 transcript 可自動 resolve
11. 非 workspace member 不可讀 learning candidates / timeline

驗證命令：

```bash
cd product/services/backend
alembic upgrade head
python -m pytest \
  tests/test_project_identity_resolution.py \
  tests/test_meeting_todo_reconciliation.py \
  tests/test_learning_candidates.py \
  tests/test_meeting_learning_api.py \
  -q
```

Frontend：

```bash
cd product/apps/frontend
npm run lint
npm run build
```

---

## 9. 建議拆 3 個 PR

### PR1：資料模型與 Project Identity

- migration
- `meeting_projects`
- `meeting_project_aliases`
- `meeting_project_mentions`
- `learning_candidates`
- `task_events`
- `project_identity.py`
- unit tests

### PR2：Meeting Learning Pipeline

- `meeting_learning.py`
- `todo_reconciliation.py`
- AIJob job types
- Hermes structured schema
- meeting learning API
- backend tests

### PR3：Frontend Review UI

- transcript 分析入口
- AIJob 狀態顯示
- project mention review
- ToDo diff review
- candidate approve / reject
- project timeline

---

## 10. 明確不要做

請 Claude Code 避免：

- 不要每次 transcript 都新增一批新 ToDo。
- 不要只靠 raw project name 建專案。
- 不要 ambiguous project 自動 merge。
- 不要沒有 citation 就 apply task change。
- 不要讓 AI 自動永久改 memory / rule。
- 不要讓 Hermes 直接讀 DB / storage。
- 不要另開一套平行 job system。
- 不要一開始導入 vector DB / 完整 RAG。

---

## 給 Claude Code 的任務摘要

請基於目前 `product/` monorepo 的 FastAPI backend + Next.js frontend，實作一套可審核的 Hermes Meeting Learning Loop，用於將會議逐字稿 / 錄音轉文字維護成有歷史脈絡的專案 ToDo。系統需先做 project identity resolution，將 transcript 中不同名稱、簡稱、別名解析到 canonical meeting project；再載入該專案既有 tasks、task history、approved aliases 與 project memory，讓 Hermes 產生 `create/update/complete/reopen/supersede/no_change/needs_clarification` 等 ToDo change proposals，而不是每次只新增新 ToDo。所有 alias、task change、memory update 都必須先寫入 `learning_candidates`，經人工 review 後才 apply。每個 task proposal 必須附 transcript citation；模糊專案解析不可自動合併。
