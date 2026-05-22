# Mac Mini Agent Sync Architecture

## 目標

Mac Mini 作為私有遠端 AI Agent 運算伺服器，負責呼叫 Hermes / OpenClaw、執行後端任務、Git 操作、檔案處理與長時間工作；但使用者資料主權、金鑰、短長期記憶、權限與敏感資訊控管必須以使用者為邊界。

## 現有程式已具備的基礎

- Rust Axum backend + PostgreSQL + sqlx。
- OpenClaw / Hermes 以 OpenAI-compatible `/v1/chat/completions` API 接入。
- `organizations / workspaces / project_acl` 與 `user_can_access_project()` 已形成三層 ACL。
- `VaultService` 已有 User KEK + System KEK recovery 的 envelope encryption 架構。
- `SessionKeyStore` 在登入後把 User KEK 保存在 RAM，登出 / 過期 / 重啟後失效。
- `DMG_ROOT` + `PROJECT_DATA_ROOT` 已支援每使用者 APFS encrypted sparse image：`<PROJECT_DATA_ROOT>/users/<user_id>/`。
- Git clone / upload 目前已落在 `PROJECT_DATA_ROOT/users/<user_id>/...`。
- Context Firewall 已能依 agent policy 過濾 code context、project memory、history，並做 redaction / audit。

## 架構原則

### 1. Mac Mini 是運算節點，不是資料所有者

Mac Mini 可以暫存與處理資料，但資料必須綁定：

```text
owner_user_id
organization_id
workspace_id
project_id
source_device_id
classification
encryption_state
```

所有讀取、下載、Git 操作、AI context 組裝都必須先通過 ACL 與 vault/session gate。

### 2. 使用者同步通道與 AI 任務通道分離

建議分兩條通道：

```text
User Device ⇄ Kway Backend API / WebSocket ⇄ Mac Mini
Kway Backend ⇄ OpenClaw / Hermes Gateway ⇄ AI Agent Runtime
```

使用者裝置只與 Kway Backend 溝通；Backend 負責授權、解密、脫敏、組 context 後才呼叫 Hermes/OpenClaw。

### 3. 檔案同步必須進使用者私有工作區

落地路徑：

```text
PROJECT_DATA_ROOT=/Users/kwayrdc/kway-project-data
DMG_ROOT=/Users/kwayrdc/kway-dmg-store

使用者檔案：
/Users/kwayrdc/kway-project-data/users/<user_id>/...

加密 DMG：
/Users/kwayrdc/kway-dmg-store/<user_id>.sparseimage
```

登入時掛載；登出時卸載。AI Agent 只處理該使用者已授權且已掛載的工作目錄。

### 4. KEK / DEK 是任務執行前置條件

- User KEK：由使用者密碼 + `users.kek_salt` 派生，只存在 RAM。
- File/Object DEK：每個物件一把，包在 `vault_key_wrappings`。
- 正常操作：User KEK unwrap DEK。
- Recovery：System KEK unwrap DEK，必須 audit。

Agent 不應持有永久金鑰；只取得後端提供的短期、授權後明文片段或工具執行結果。

### 5. Git 操作是受控資料異動

Git identity、repo、branch、local path 必須與 user/project ACL 綁定。每次 AI Git 操作需記錄：

```text
actor_user_id
agent_name
project_id
source_id / repo_path
operation
branch
files_changed
credential_ref
result
timestamp
```

現有 `git_identities.access_token` 仍有 System KEK fallback，若要符合最嚴格資料主權，建議改為只允許 User KEK session path，除非走明確 recovery 流程。

## Mac Mini 建議設定

### OpenClaw Gateway

安全拓撲：Gateway 只綁本機，由 Tailscale Serve 對 tailnet 提供 HTTPS。

```bash
openclaw config set gateway.bind loopback
openclaw config set gateway.tailscale.mode serve
openclaw gateway restart
```

對外入口：

```text
https://kwayrdcmac-mini.tail315af3.ts.net/
```

本機入口：

```text
http://127.0.0.1:18789/
```

不要使用 `--bind lan --tailscale serve`；OpenClaw Serve 模式要求 loopback。

### Kway Backend `.env`

建議 Mac Mini production-like 設定：

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

若前端也要經 Tailscale 對外，建議用 reverse proxy / Tailscale Serve 對 backend + web 分路，而不是直接把 `8080` 暴露到 LAN 或公網。

## 建議新增能力

### A. Device Sync Channel

新增裝置註冊與同步表：

```text
user_devices
sync_sessions
sync_events
file_sync_objects
```

用途：使用者本機與 Mac Mini 雙向同步檔案、狀態、任務結果。

### B. Agent Task Broker

新增任務中心：

```text
agent_tasks
agent_task_events
agent_task_artifacts
```

流程：

```text
User request
→ create agent_task
→ ACL + User KEK session check
→ mount user DMG if needed
→ prepare authorized workspace
→ call Hermes/OpenClaw
→ write artifact / patch / result
→ notify user device via WebSocket
```

### C. Encrypted File Workspace Registry

現有檔案多落在 project path，但缺少通用 registry。建議新增：

```text
workspace_files
file_versions
file_access_audit
```

用來追蹤所有 upload/download/Git/AI artifact 的 owner、path、hash、classification、encryption state。

### D. Strict Git Credential Mode

把 git identity 從「User KEK 優先、System KEK fallback」改為：

```text
normal operation: require active User KEK
admin recovery: explicit recovery endpoint / CLI + audit
```

這更符合「各使用者本機端控管」的原則。

## 與 Meeting Learning Loop 的關係

Meeting Learning Loop 應該是上層應用，不是底層架構。正確順序：

1. Mac Mini private agent server + secure sync channel
2. Per-user encrypted workspace + ACL + audit
3. Agent task broker
4. Context firewall / memory review
5. Meeting Learning Loop

若先做 Meeting Learning，容易把資料、記憶、任務狀態混在同一層，後續會難以滿足使用者隔離與加密要求。

## 目前風險

- Docker compose 的 backend `PROJECT_DATA_ROOT=/app/data` volume 無法直接利用 macOS per-user DMG；Mac Mini production 建議先用 host-run backend 或 bind mount 到 `/Users/kwayrdc/kway-project-data`。
- Git credential 仍有 System KEK fallback，嚴格模式下需收斂。
- 大檔 vault path 在 `VaultService.open()` 仍標示 large-object path not yet implemented；大型檔案應以 encrypted DMG + file registry 為主，vault 只保護 DEK / secret。
- OpenClaw / Hermes 目前是後端 agent client，不等於多使用者任務佇列；需要 Agent Task Broker 才能可靠雙向同步狀態。
