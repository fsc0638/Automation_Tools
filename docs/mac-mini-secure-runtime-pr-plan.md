# Mac Mini Secure Runtime Foundation — PR Plan

## 背景

Kway Dev 的目標不是單純的會議或 ToDo 系統，而是以 Mac Mini 作為私有遠端 AI Agent 運算伺服器：

- Mac Mini 負責 Hermes / OpenClaw 後端運算、任務處理、Git 操作、檔案處理與長時間工作。
- 使用者資料主權、權限、短長期記憶、金鑰與敏感資訊控管必須以使用者為邊界。
- 所有上傳、下載、Git、AI 任務產物都要落在 Mac Mini，但每位使用者只能瀏覽與異動自己有權限的檔案。
- 檔案可以在使用者本機經 KEK / DEK 加密後同步到 Mac Mini，AI Agent 執行任務時只能透過授權邊界取得必要資料。

參考架構文件：

```text
docs/mac-mini-agent-sync-architecture.md
```

---

## PR1 — Mac Mini Runtime / Deployment Baseline

### 目標

建立 Mac Mini 作為安全 AI Agent 運算節點的部署基線。

### Scope

- 新增 Mac Mini deployment 文件。
- 更新 backend `.env.example`，加入 production-like 設定範例。
- 明確區分以下服務角色：
  - OpenClaw Gateway
  - Hermes Gateway
  - Kway Backend
  - Web frontend
  - PostgreSQL
- 說明 Docker 模式與 macOS per-user DMG 的限制。

### 建議檔案

```text
docs/mac-mini-deployment.md
backend/.env.example
docker-compose.yml
```

### 建議設定

```env
SERVER_HOST=127.0.0.1
SERVER_PORT=8080
PROJECT_DATA_ROOT=/Users/kwayrdc/kway-project-data
DMG_ROOT=/Users/kwayrdc/kway-dmg-store
DMG_SIZE_MB=4096

OPENCLAW_API_URL=http://127.0.0.1:18789/v1
OPENCLAW_MODEL=gpt-5.5

HERMES_API_URL=http://127.0.0.1:8642/v1
HERMES_MODEL=hermes-agent

GIT_TOKEN_ENCRYPTION_KEY=<base64-32-byte-key>
JWT_SECRET=<strong-random-secret>
CORS_ALLOWED_ORIGINS=https://kwayrdcmac-mini.tail315af3.ts.net
```

### 驗收條件

- 文件可清楚說明 Mac Mini 如何作為遠端 AI 運算節點。
- OpenClaw Gateway 維持 `loopback + Tailscale Serve`。
- 不要求直接把 backend / gateway 暴露到公網。
- 明確標示 Docker volume 無法直接等同 macOS per-user DMG production path。

---

## PR2 — User Workspace File Registry

### 目標

讓所有 upload / download / Git / AI artifact 都有 owner、workspace、project、path、classification、encryption state 與 audit metadata。

### DB Migration

```text
backend/migrations/0044_workspace_file_registry.sql
```

### 新增資料表

```sql
CREATE TABLE workspace_files (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    owner_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    workspace_id UUID NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    source_type TEXT NOT NULL CHECK (source_type IN ('upload','git','agent_artifact','portal','manual','meeting_file')),
    logical_path TEXT NOT NULL,
    storage_path TEXT NOT NULL,
    classification TEXT NOT NULL DEFAULT 'internal'
        CHECK (classification IN ('public','internal','confidential','secret')),
    encryption_state TEXT NOT NULL DEFAULT 'dmg'
        CHECK (encryption_state IN ('dmg','vault','plaintext_dev')),
    content_hash TEXT,
    size_bytes BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_workspace_files_owner ON workspace_files(owner_user_id, updated_at DESC);
CREATE INDEX idx_workspace_files_workspace ON workspace_files(workspace_id, updated_at DESC);
CREATE INDEX idx_workspace_files_project ON workspace_files(project_id, updated_at DESC);

CREATE TABLE file_versions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    file_id UUID NOT NULL REFERENCES workspace_files(id) ON DELETE CASCADE,
    version INT NOT NULL,
    content_hash TEXT NOT NULL,
    size_bytes BIGINT NOT NULL DEFAULT 0,
    storage_path TEXT NOT NULL,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(file_id, version)
);

CREATE TABLE file_access_audit (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    file_id UUID NOT NULL REFERENCES workspace_files(id) ON DELETE CASCADE,
    actor_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    operation TEXT NOT NULL CHECK (operation IN (
        'upload','download','read','write','delete','agent_read','agent_write','git_clone','git_pull','git_checkout'
    )),
    agent_name TEXT,
    task_id UUID,
    ip_addr TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_file_access_audit_file ON file_access_audit(file_id, created_at DESC);
CREATE INDEX idx_file_access_audit_actor ON file_access_audit(actor_user_id, created_at DESC);
```

### Backend

新增：

```text
backend/src/api/workspace_files.rs
backend/src/file_registry.rs
```

整合點：

```text
backend/src/api/projects.rs        # project upload / git clone 後登記 workspace_files
backend/src/api/meetings.rs        # meeting_files 登記 workspace_files
backend/src/git_ops/manager.rs     # git operation audit hook
backend/src/api/mod.rs             # merge workspace_files routes
```

### API 草案

```http
GET /workspace-files?workspace_id=<id>&project_id=<id>
GET /workspace-files/:id
GET /workspace-files/:id/versions
GET /workspace-files/:id/audit
```

### 驗收條件

- 每個使用者檔案可追 owner / workspace / project。
- 非 owner 且無 ACL 者不可讀。
- AI 讀寫檔案會留下 `file_access_audit`。
- 既有 project upload / git clone 不改變使用者流程，但背後會登記 registry。

---

## PR3 — Device Sync Channel

### 目標

建立使用者本機裝置與 Mac Mini 的雙向同步控制層。第一版先做事件與狀態同步，不做 conflict merge。

### DB Migration

```text
backend/migrations/0045_device_sync_channel.sql
```

### 新增資料表

```sql
CREATE TABLE user_devices (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_name TEXT NOT NULL,
    device_type TEXT NOT NULL CHECK (device_type IN ('mac','windows','ios','android','web','unknown')),
    public_key TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','revoked')),
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_user_devices_user ON user_devices(user_id, updated_at DESC);

CREATE TABLE sync_sessions (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id UUID NOT NULL REFERENCES user_devices(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','expired','revoked')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_sync_sessions_user ON sync_sessions(user_id, created_at DESC);

CREATE TABLE sync_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id UUID REFERENCES user_devices(id) ON DELETE SET NULL,
    event_type TEXT NOT NULL,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    acked_at TIMESTAMPTZ
);

CREATE INDEX idx_sync_events_user ON sync_events(user_id, created_at DESC);
CREATE INDEX idx_sync_events_pending ON sync_events(user_id, created_at DESC) WHERE acked_at IS NULL;
```

### Backend

新增：

```text
backend/src/api/devices.rs
backend/src/api/sync.rs
```

API 草案：

```http
POST /devices/register
GET  /devices
POST /devices/:id/revoke
GET  /sync/events
POST /sync/events/:id/ack
```

### 驗收條件

- 使用者裝置可以註冊。
- 每個 sync event 綁定 user_id / device_id。
- 任務狀態、檔案變更可以先寫入 sync_events。
- 前端或本機 client 可查詢 pending events。

---

## PR4 — Agent Task Broker Skeleton

### 目標

所有 Hermes / OpenClaw 任務進入統一任務中心，避免 AI 任務邏輯散落在各 API handler。

### DB Migration

```text
backend/migrations/0046_agent_task_broker.sql
```

### 新增資料表

```sql
CREATE TABLE agent_tasks (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    requested_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    organization_id UUID REFERENCES organizations(id) ON DELETE SET NULL,
    workspace_id UUID REFERENCES workspaces(id) ON DELETE SET NULL,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    agent_target TEXT NOT NULL CHECK (agent_target IN ('openclaw','hermes','debate')),
    task_type TEXT NOT NULL CHECK (task_type IN ('chat','git','file_process','meeting_learning','system')),
    status TEXT NOT NULL DEFAULT 'queued'
        CHECK (status IN ('queued','running','waiting_user','completed','failed','cancelled')),
    input_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    result_json JSONB,
    error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_agent_tasks_user ON agent_tasks(requested_by, created_at DESC);
CREATE INDEX idx_agent_tasks_project ON agent_tasks(project_id, created_at DESC);
CREATE INDEX idx_agent_tasks_status ON agent_tasks(status, created_at DESC);

CREATE TABLE agent_task_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    task_id UUID NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    message TEXT,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_agent_task_events_task ON agent_task_events(task_id, created_at DESC);

CREATE TABLE agent_task_artifacts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    task_id UUID NOT NULL REFERENCES agent_tasks(id) ON DELETE CASCADE,
    file_id UUID REFERENCES workspace_files(id) ON DELETE SET NULL,
    artifact_type TEXT NOT NULL CHECK (artifact_type IN ('patch','report','generated_file','log','json')),
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_agent_task_artifacts_task ON agent_task_artifacts(task_id, created_at DESC);
```

### Backend

新增：

```text
backend/src/agent_tasks/mod.rs
backend/src/agent_tasks/broker.rs
backend/src/agent_tasks/executor.rs
backend/src/api/agent_tasks.rs
```

API 草案：

```http
POST /agent-tasks
GET  /agent-tasks
GET  /agent-tasks/:id
GET  /agent-tasks/:id/events
GET  /agent-tasks/:id/artifacts
POST /agent-tasks/:id/cancel
```

### 初版限制

- 初版只包裝 chat 類 Hermes / OpenClaw 任務。
- 不一次重構現有 meeting notes generation。
- 後續再把 meeting learning / git / file_process 遷入 broker。

### 驗收條件

- 可建立 agent task。
- 有狀態流轉：queued → running → completed / failed。
- 任務事件會寫入 `agent_task_events`。
- 任務結果可查。
- 任務完成後可寫入 `sync_events` 通知使用者裝置。

---

## PR5 — Strict User KEK Gate for Git / Sensitive File Ops

### 目標

讓 Git 與敏感檔案操作符合使用者資料主權：正常 API path 必須有 active User KEK，不再靜默 fallback 到 System KEK。

### 變更點

目前 `identity_credentials()` 在沒有 User KEK 或 User KEK 解密失敗時，可能 fallback 到 System KEK。此 PR 改為：

```text
normal API operation: require active User KEK
admin recovery: explicit CLI / recovery path only + audit
```

### 影響檔案

```text
backend/src/api/git_identities.rs
backend/src/api/projects.rs
backend/src/api/vault.rs
backend/src/security/vault_service.rs
backend/src/security/session_keys.rs
```

### 行為規則

- Git clone / pull / checkout 若需要 token，必須要求 active User KEK。
- server restart 後，使用者需重新登入才能操作 Git token / encrypted file。
- System KEK 不可在一般 API path 透明 fallback。
- Recovery 必須透過 admin CLI 或明確 endpoint，且寫入 audit。

### 驗收條件

- 無 active User KEK 時，Git 操作回：

```text
session expired, please log in again
```

- 其他使用者不能透過 System KEK fallback 讀取資料。
- recovery path 會寫 `vault_audit_log`。

---

## PR6 — Agent Context Authorization Boundary

### 目標

Hermes / OpenClaw 只收到經授權、脫敏、分類過的 context；Agent 不直接碰 DB / storage。

### 變更點

擴充 Context Firewall：

- file registry ACL check
- file classification check
- agent data policy check
- redaction
- outbound context hash audit
- blocked files audit

### 影響檔案

```text
backend/src/security/context_firewall.rs
backend/src/security/redaction.rs
backend/src/agents/orchestrator.rs
backend/src/grounding/mod.rs
backend/src/grounding/tools.rs
backend/src/api/ws.rs
```

### 建議新增欄位 / 表

可先沿用既有 `agent_context_audit_logs`，必要時新增：

```text
agent_authorized_contexts
```

欄位草案：

```text
id
task_id nullable
user_id
project_id
agent_mode
included_file_ids jsonb
blocked_file_ids jsonb
classification_max
outbound_context_hash
created_at
```

### 驗收條件

- Agent prompt 不含未授權檔案。
- Secret / token 會被 redacted。
- Debate 模式取最嚴格 policy。
- 每次 agent call 有 context hash 與 audit metadata。

---

## 建議開發順序

```text
PR1  Mac Mini Runtime / Deployment Baseline
PR2  User Workspace File Registry
PR3  Device Sync Channel
PR4  Agent Task Broker Skeleton
PR5  Strict User KEK Gate for Git / File Ops
PR6  Agent Context Authorization Boundary
```

---

## 暫緩項目

以下項目先不要進第一階段：

- Meeting Learning Loop
- Vector DB / 完整 RAG
- 自動 retrain
- 多人共用記憶
- 跨裝置 conflict merge
- Agent 自動永久修改 memory / rules

原因：資料主權、同步、任務邊界尚未穩定前，先做上層 learning 會增加重構成本。

---

## 第一階段完成定義

完成 PR1–PR6 後，系統應具備：

1. Mac Mini 作為安全 AI Agent 運算節點。
2. 每位使用者有獨立 encrypted workspace。
3. 檔案、Git、AI artifact 都有 registry 與 audit。
4. 使用者裝置能與 Mac Mini 雙向同步任務狀態。
5. Hermes / OpenClaw 任務進入統一 task broker。
6. Git / sensitive file 操作要求 active User KEK。
7. Agent context 經 ACL / classification / redaction / audit 後才送出。
