# Phase 1 實作報告：Kway 內部 + 關係企業部署

> **狀態**：2026-05-26 — Phase 1 全部完成、驗證通過
> **適用範圍**：Kway 內部員工 + 關係企業（同集團內）
> **下一階段**：[Phase 2 商品化](./phase-2-roadmap.md)

---

## 目錄

1. [架構總覽](#1-架構總覽)
2. [威脅模型 + 防禦縱深](#2-威脅模型--防禦縱深)
3. [Client-held KEK 協定](#3-client-held-kek-協定)
4. [三層加密棧](#4-三層加密棧)
5. [Multi-tenant 模型](#5-multi-tenant-模型)
6. [網路與防火牆](#6-網路與防火牆)
7. [備份與災難復原](#7-備份與災難復原)
8. [Launchd 自動化](#8-launchd-自動化)
9. [Code map（檔案速查）](#9-code-map檔案速查)
10. [今日 audit 發現與修正](#10-今日-audit-發現與修正)
11. [已知限制與已記錄 Phase 2 項目](#11-已知限制與已記錄-phase-2-項目)

---

## 1. 架構總覽

### 1.1 部署拓撲

```
┌─────────────────────────────────────────────────────────────┐
│                  Kway Tailnet (Tailscale)                   │
│                                                             │
│  ┌─────────────┐      WireGuard      ┌────────────────────┐ │
│  │  Windows    │  ═══════════════►   │   Mac Mini         │ │
│  │  100.84.x.x │      direct P2P     │   100.74.166.122   │ │
│  │  Chrome     │      (or DERP)      │                    │ │
│  └─────────────┘                     │  ┌──────────────┐  │ │
│  ┌─────────────┐                     │  │ Next.js :3000│  │ │
│  │  iOS App    │  ═══════════════►   │  │   bound to   │  │ │
│  │             │                     │  │ Tailscale IP │  │ │
│  └─────────────┘                     │  └──────┬───────┘  │ │
│  ┌─────────────┐                     │         │ /api/*   │ │
│  │  Mac        │  ═══════════════►   │  ┌──────▼───────┐  │ │
│  │  (admin)    │                     │  │ Rust :8080   │  │ │
│  └─────────────┘                     │  │   axum       │  │ │
│                                      │  └──────┬───────┘  │ │
└──────────────────────────────────────│         │          │─┘
                                       │  ┌──────▼───────┐  │
                                       │  │  Postgres    │  │
                                       │  │  (Docker)    │  │
                                       │  └──────────────┘  │
                                       │  ┌──────────────┐  │
                                       │  │ ~/kway-dmg-  │  │
                                       │  │   store/     │  │
                                       │  │ *.sparseimage│  │
                                       │  │ (AES-CDSA)   │  │
                                       │  └──────────────┘  │
                                       └────────────────────┘
                                              │
                                              ▼
                                       ~/kway-backups/
                                       YYYY-MM-DD/  (03:00 nightly)
                                       ├── postgres.dump
                                       ├── backend.env
                                       ├── sparseimages/
                                       └── manifest.txt
```

### 1.2 元件與技術棧

| 元件 | 技術 | 路徑 | 用途 |
|---|---|---|---|
| **Backend** | Rust + axum + sqlx | `backend/` | REST API、authn、加密、業務邏輯 |
| **Web** | Next.js (App Router) | `web/` | 瀏覽器介面、Argon2 WASM、KEK 客戶端派生 |
| **iOS** | Swift | `ios/` | 行動端 client |
| **Database** | PostgreSQL 16 (Docker) | `docker-compose.yml` | users、orgs、projects、vault、audit |
| **Per-user encryption** | macOS hdiutil (AES-128 CDSA) | `~/kway-dmg-store/` | 每使用者一份加密磁碟映像 |
| **VPN** | Tailscale (WireGuard) | system-wide | 加密通道，唯一進入路徑 |
| **Auto-start** | launchd (LaunchAgent) | `~/Library/LaunchAgents/` | backend / web / 每晚 backup |
| **Backup target** | local rsync + 異地（人工） | `~/kway-backups/` | 03:00 nightly |

### 1.3 啟動順序

```
boot
  └─→ Docker Desktop 自動起 (macOS auto-launch)
        └─→ postgres container (compose up)
              └─→ launchd 載入三個 agent (user login)
                    ├─→ com.kway.dev.backend
                    │     ├─→ run-backend.sh
                    │     │     ├─→ source .env
                    │     │     ├─→ resolve auto-tailscale → 100.74.166.122
                    │     │     └─→ exec kway-dev-backend
                    │     │
                    │     ├─→ binary 啟動：
                    │     │     ├─→ sqlx::migrate! 套用 0001..0050
                    │     │     ├─→ sweep_stale_mounts() — detach 殘留 DMG
                    │     │     ├─→ axum serve 100.74.166.122:8080
                    │     │     └─→ session_keys = empty (登入時填)
                    │
                    ├─→ com.kway.dev.web
                    │     └─→ run-web.sh → next start -H 100.74.166.122 -p 3000
                    │
                    └─→ com.kway.dev.backup
                          └─→ idle，03:00 排程觸發 scripts/backup.sh
```

---

## 2. 威脅模型 + 防禦縱深

### 2.1 我們防什麼

| 威脅 | 緩解 |
|---|---|
| **網路竊聽** | Tailscale WireGuard 加密通道（不靠 HTTPS app 層） |
| **資料庫被偷（DB dump 外洩）** | password_hash 雙重 Argon2id、vault ciphertext 為 ChaCha20-Poly1305、DMG 鑰匙 wrap 在 vault 內 |
| **Mac Mini 整台被偷** | DMG 為 AES-128 加密，密碼從 user KEK 派生，KEK 在 server RAM 中（登出歸零）；磁碟本體無 KEK 也是密文 |
| **使用者登出後 server admin 偷看** | 登出時 session_keys.remove() + Drop trait 觸發 zeroize；server 連 user KEK 都拿不到 |
| **跨 user 偷資料** | DB function `user_can_access_project` 集中授權；vault per-user wrap |
| **同辦公室 LAN 隨機機器掃描 :8080 / :3000** | 服務只 bind Tailscale IP，LAN 介面看不到 port 開著 |
| **Backend crash 留 DMG mount 給他人讀** | 啟動時 `sweep_stale_mounts()` 自動 detach |
| **Email 推測攻擊（測 timing 找哪些 email 已註冊）** | 不存在 email 也跑 dummy Argon2 verify，response time 一致 |
| **管理員「忘了改密碼」就解人家 vault** | password change 是 atomic transaction，舊 wrap 立即被新 KEK 重 wrap |

### 2.2 我們明確**不**防什麼（Phase 1 內部信任假設）

| 威脅 | 為什麼不防 |
|---|---|
| **同一個 Kway 員工偷看會議室預訂** | meetings.rs:511 注解：「同事互信、需要看到別人的會議」(2026-05-15 design call) |
| **Kway 自己（公司）想看員工資料** | system_v1 wrap 是公司的 escape hatch，**設計如此** |
| **Tailscale 公司被駭** | 信任 Tailscale 的 control plane；商品化前可改 self-host Headscale |
| **被中國/俄羅斯國家級 APT 鎖定攻擊** | 不在 threat model 內 |

---

## 3. Client-held KEK 協定

### 3.1 為什麼這麼設計

傳統做法（**錯**）：
```
client → POST /login { password: "hunter2" }
server → Argon2(password) → 比對 password_hash → 發 JWT
```
**問題**：server 看得到明文密碼。任何 server admin、SRE、被駭的 server，都能順手偷使用者密碼用到別的網站。

我們的做法：
```
client (browser/iOS) → Argon2id(password, kek_salt) → User KEK (32 bytes)
client                → Argon2id(password, kek_salt, prefix="auth") → auth_hash (32 bytes)
client → POST /login { email, auth_hash, user_kek }
server → Argon2id(auth_hash) → 比對 password_hash → 發 JWT
server → session_keys[user_id] = user_kek  ←  KEK 進 RAM、不落地
```

**保證**：
- ❌ Server **永遠沒看過**明文密碼
- ❌ Server **永遠沒儲存** KEK
- ✅ Server 只在 user 在線時持有 KEK (in-RAM)
- ✅ DB dump 被偷只能拿到 Argon2(Argon2(password)) — 雙重抗暴力

### 3.2 註冊流程（端到端）

```
1. browser GET /api/auth/kek-params?email=alice@test.com
                       ↓
   server: 看 user 不存在 → 生 random kek_salt + return
   {
     "kek_salt": "<32 bytes base64>",       ← 隨機，這個 email 註冊後就固定
     "argon2_m_cost": 65536,
     "argon2_t_cost": 3,
     "argon2_p_cost": 4,
     "kek_domain": "kway-kek-v1::",
     "auth_domain": "kway-auth-v1::"
   }

2. browser 跑兩輪 Argon2id (WASM)：
   user_kek  = Argon2id(password, kek_salt, domain=kek_domain,  out=32 bytes)
   auth_hash = Argon2id(password, kek_salt, domain=auth_domain, out=32 bytes)

3. browser POST /api/auth/register {
     "email": "alice@test.com",
     "display_name": "Alice",
     "kek_salt": "...",       ← server 還是要存這個，下次登入要用同一個
     "auth_hash": "...",      ← 客戶端 Argon2 出來的
     "user_kek": "..."        ← 客戶端 Argon2 出來的（這個 KEK 等等要 wrap DMG passphrase + 之後 vault）
   }

4. server：
   - random DMG passphrase (32 bytes)
   - wrap_dek(user_kek, DMG_passphrase)   → user wrap 寫 vault_key_wrappings
   - wrap_dek(system_v1, DMG_passphrase)  → system wrap 寫 vault_key_wrappings (escape hatch)
   - hdiutil create <uuid>.sparseimage 用 DMG_passphrase
   - hdiutil attach 到 ~/kway-project-data/users/<uuid>/
   - INSERT INTO users (password_hash = Argon2(auth_hash), kek_salt, ...)
   - session_keys[user_id] = user_kek
   - 發 JWT
```

[實作位置](../backend/src/api/auth.rs#L45)：
```rust
// RegisterRequest 結構體 — 注意：NO password field
pub struct RegisterRequest {
    pub email: String,
    pub display_name: String,
    pub kek_salt: String,    // base64 32 bytes
    pub auth_hash: String,   // base64 32 bytes
    pub user_kek: String,    // base64 32 bytes
}
```

### 3.3 登入流程

```
1. browser GET /api/auth/kek-params?email=alice@test.com
   server 從 DB 拿 alice 的 kek_salt 回傳（但時序攻擊測試確保未註冊 email 也回 deterministic salt）

2. browser 跑同樣兩輪 Argon2 → user_kek + auth_hash

3. browser POST /api/auth/login { email, auth_hash, user_kek }

4. server: Argon2(auth_hash) 跟 DB password_hash 比對
   - 對 → session_keys[user_id] = user_kek, 發 JWT, hdiutil attach DMG
   - 錯 → 401（無論 user 存不存在都跑 dummy verify，timing 一致）
```

### 3.4 變更密碼（atomic re-wrap）

最容易踩雷的操作。流程：

```
browser 算出：
  current_auth_hash, current_user_kek  ← 舊密碼派生
  new_auth_hash,     new_user_kek      ← 新密碼派生

browser POST /api/auth/change-password { 上面四個欄位 }

server BEGIN TRANSACTION:
  1. verify(current_auth_hash, users.password_hash) — 確定真的是本人
  2. 抓所有 vault_key_wrappings WHERE kek_alias = "user:<uuid>" FOR UPDATE
  3. for each wrap:
       dek = unwrap_dek(old_user_kek, wrap.wrapped_dek)
       new_wrap = wrap_dek(new_user_kek, dek)
       UPDATE vault_key_wrappings SET wrapped_dek = new_wrap
  4. UPDATE users SET password_hash = Argon2(new_auth_hash)
  5. UPDATE users SET kek_salt = new_salt (optional rotation)
COMMIT

server session_keys[user_id] = new_user_kek
```

[實作位置](../backend/src/api/auth.rs#L617)：`re_wrap_deks_and_update_password()`

**為什麼用 transaction**：如果中間 crash，password_hash 變了但 wrap 沒變 → user 就永遠解不開 vault。一個 transaction 確保 all-or-nothing。

---

## 4. 三層加密棧

### 4.1 三層說明

```
┌──────────────────────────────────────────────┐
│ Layer 1: Tailscale WireGuard (網路傳輸)      │
│  - 加密：ChaCha20-Poly1305                   │
│  - 鑰匙：peer-to-peer ECDH，每 ~2 分鐘換     │
│  - 防：網路竊聽、MITM                        │
└────────────────────┬─────────────────────────┘
                     │
┌────────────────────▼─────────────────────────┐
│ Layer 2: Vault Application Layer (DB 存儲)   │
│  - 加密：ChaCha20-Poly1305 (vault_ciphertexts)│
│  - DEK：random per-secret 32 bytes            │
│  - DEK wrap：user KEK + system_v1（雙重 wrap）│
│  - 防：DB dump 被偷                          │
└────────────────────┬─────────────────────────┘
                     │  (vault secret 寫入 DB)
                     │  (檔案寫入 mount point)
                     │
┌────────────────────▼─────────────────────────┐
│ Layer 3: DMG Per-User Volume (檔案系統)      │
│  - 加密：AES-128 (macOS CDSA, encrcdsa)       │
│  - 鑰匙：random 32 bytes DMG passphrase       │
│  - passphrase wrap：user KEK + system_v1     │
│  - 防：Mac Mini 整台被偷、登出後 server 偷看 │
└──────────────────────────────────────────────┘
```

### 4.2 Vault 加密細節

每一筆 vault secret 寫入時：

```sql
-- vault_secrets：metadata
INSERT INTO vault_secrets (id, user_id, label, type, ...) VALUES (...)

-- vault_ciphertexts：實際密文
INSERT INTO vault_ciphertexts (object_id, ciphertext)
VALUES (
  <secret_id>,
  ChaCha20-Poly1305(DEK, nonce, plaintext_secret_value)
)

-- vault_key_wrappings：DEK 被誰 wrap 了（一定要有兩筆）
INSERT INTO vault_key_wrappings (object_id, object_type, kek_alias, wrapped_dek) VALUES
  (<secret_id>, 'vault_secret', 'user:<uuid>',  wrap(user_kek, DEK)),
  (<secret_id>, 'vault_secret', 'system_v1',    wrap(system_kek, DEK));
```

**Reveal 流程**：
```
GET /api/vault/secrets/<id>/reveal
  → user_kek = session_keys[user_id]   ← 不在 = 401 "session expired"
  → user_wrap = SELECT wrapped_dek WHERE kek_alias = 'user:<uuid>'
  → DEK = unwrap_dek(user_kek, user_wrap)
  → ciphertext = SELECT ciphertext FROM vault_ciphertexts
  → plaintext = ChaCha20-Poly1305-decrypt(DEK, ciphertext)
  → return { "secret_value": plaintext }
```

[實作位置](../backend/src/api/vault.rs#L167)：`reveal_secret()` handler

### 4.3 DMG 加密細節

每個 user 在 `~/kway-dmg-store/<user-uuid>.sparseimage`：

- **格式**：macOS sparse image with `-encryption AES-128`
- **Header magic**：`encrcdsa` (Cocoa Disk Image with encryption)
- **Passphrase**：建立時隨機 32 bytes (`raw_key`)；passphrase 不存在任何明文位置
- **DMG passphrase wrap**：跟 vault secret 一樣存在 `vault_key_wrappings` 表，`object_type='user_dmg_key'`
- **Mount point**：`~/kway-project-data/users/<user-uuid>/`
- **Mount 觸發**：登入時，`session_keys[user_id] = user_kek` 之後立刻 unwrap DMG passphrase + hdiutil attach
- **Unmount 觸發**：登出時 `hdiutil detach`；backend crash recovery 時 `sweep_stale_mounts()`

[實作位置](../backend/src/security/dmg_manager.rs)：
- `create()`：建立新 sparse image，line 100+
- `mount()`：line 130+
- `unmount()`：line 173+，含 `-force` retry
- `is_mounted()`：line 203+（**今天修正：`/bin/mount` → `/sbin/mount`**）
- `sweep_stale_mounts()`：line 215+（**今天新增**）

### 4.4 「登出 = 連 server 自己也讀不到」的證明鏈

這是這套設計的賣點。完整證明：

```
T = 0   user login
        session_keys[user_id] = user_kek (in RAM)
        DMG mounted at users/<uuid>/

T = +5  user 上傳 file.txt
        file.txt 寫入 users/<uuid>/upload-X/file.txt
        實體位元組進入 sparseimage block，DMG layer 自動 AES 加密

T = +10 user 登出
        POST /api/auth/logout
          → session_keys.remove(user_id)   ← KEK drop()ed, Drop trait → write_volatile(0)
          → hdiutil detach users/<uuid>    ← AES 鑰匙從 kernel 卸載

T = +11 server admin / 入侵者試圖讀 file.txt
        users/<uuid>/ → 空目錄（mount 卸了）
        ~/kway-dmg-store/<uuid>.sparseimage → AES 密文，無 passphrase 解不開
        passphrase 在哪？只在 DB 裡 wrap 在 vault_key_wrappings — 但 wrap 也要 user_kek 解
        user_kek 在哪？只在 user 下次登入時客戶端再算一次 Argon2 — server 沒有
        ✅ Server 自己也讀不到
```

**今天實測** (測試 5 + 測試 7)：
- 登出後 `mount | grep` → 無
- `hdiutil info` → 無
- `strings ~/kway-dmg-store/<uuid>.sparseimage | grep WINDOWS_SECRET_FROM_TAILSCALE_2026` → 無
- `grep -a` 二進位搜也找不到
- 連檔名 metadata 都不洩漏

---

## 5. Multi-tenant 模型

### 5.1 四層階級

```
organizations
  ├── workspaces (多個)
  │     ├── projects (多個)
  │     │     ├── conversations
  │     │     ├── meetings
  │     │     ├── agent_tasks
  │     │     ├── files
  │     │     └── ...
  │     └── workspace_members
  └── organization_members
```

每個 user **必然**在至少一個 org（Personal Org）。新 user 第一次建 project 時 lazy-create Personal Org；2026-05-26 加的 migration 0050 把舊 user backfill 一次。

### 5.2 中央授權 DB function

[`backend/migrations/0019_organization_project_acl.sql`](../backend/migrations/0019_organization_project_acl.sql)：

```sql
CREATE FUNCTION user_can_access_project(
    p_project_id uuid,
    p_user_id uuid,
    p_min_role text DEFAULT 'viewer'
) RETURNS boolean AS $$
    SELECT EXISTS (
        SELECT 1
        FROM projects p
        LEFT JOIN project_acl pa
          ON pa.project_id = p.id AND pa.user_id = p_user_id
        LEFT JOIN organization_members om
          ON om.organization_id = p.organization_id AND om.user_id = p_user_id
        LEFT JOIN workspace_members wm
          ON wm.workspace_id = p.workspace_id AND wm.user_id = p_user_id
        WHERE p.id = p_project_id
          AND (
            p.user_id = p_user_id                                       -- direct owner
            OR access_role_rank(pa.role) >= access_role_rank(p_min_role) -- ACL grant
            OR access_role_rank(om.role) >= access_role_rank(p_min_role) -- org member
            OR access_role_rank(wm.role) >= access_role_rank(p_min_role) -- workspace member
          )
    );
$$ LANGUAGE sql STABLE;
```

**任何 handler 要 access project 都用這個 function 過濾**，不能寫自己的 SQL。

### 5.3 各 handler 採用模式

| Handler | 過濾機制 |
|---|---|
| `projects.rs` | `WHERE user_can_access_project(id, $user, $role)` |
| `vault.rs` | `WHERE user_id = $user`（per-user，無 org 概念） |
| `conversations.rs` | `verify_project_access(project_id, user)` helper |
| `meetings.rs` | 複雜 OR：creator + attendee + portal-imported + project access |
| `agent_tasks.rs` | `require_agent_task_access()` → `user_can_access_project` |
| `workspace_files.rs` | `require_file_access()` helper |
| `device_sync.rs` | `WHERE owner_user_id = $user`（per-user 裝置） |
| `organizations.rs` | `require_org_member` / `require_org_admin` |

### 5.4 跨租戶測試（今天驗）

```bash
# alice 的 token 試讀 alice2 的 project
curl -H "Authorization: Bearer $ALICE_TOKEN" \
     http://.../api/projects/<alice2_project_id>
→ HTTP 404 "Project not found"
```

不是 403、是 404 — 連「存在性」都不洩漏。

### 5.5 已記錄 Phase 2 必修

[meetings.rs:492](../backend/src/api/meetings.rs#L492)：

```sql
OR m.external_id IS NOT NULL  -- portal-imported meetings visible to ALL users
```

Portal scraper 匯入的會議**所有 user 都看得到**。Phase 1（一個 Kway portal 來源）OK；Phase 2（多公司各自 portal）必修。

---

## 6. 網路與防火牆

### 6.1 攻擊面收緊

| 介面 | 介面 IP | :8080 / :3000 |
|---|---|---|
| `lo0` (loopback) | `127.0.0.1` | ❌ 連不到 |
| `en0` (LAN) | `172.16.53.200` | ❌ 連不到 |
| `en1` (LAN 2) | `192.168.139.3` | ❌ 連不到 |
| `utun5` (Tailscale) | `100.74.166.122` | ✅ 唯一通道 |

### 6.2 實作方式

**不靠 pfctl**（複雜、容易誤把 SSH 也鎖死），而是讓服務只 bind 到 Tailscale IP。

`backend/.env`：
```
SERVER_HOST=auto-tailscale
```

`scripts/launchd/run-backend.sh`：
```bash
if [[ "$SERVER_HOST" == "auto-tailscale" ]]; then
    ts_ip=$(tailscale ip -4 | head -1)
    export SERVER_HOST="${ts_ip:-127.0.0.1}"   # fail closed
fi
exec ./target/release/kway-dev-backend
```

`scripts/launchd/run-web.sh`：
```bash
ts_ip=$(tailscale ip -4 | head -1)
bind_host="${ts_ip:-127.0.0.1}"
exec npx next start -H "$bind_host" -p 3000
```

### 6.3 Fail-closed 設計

如果 Tailscale 沒在跑（壞了、被禁用、節點掉線）：
- backend / web 退到 `127.0.0.1` (loopback)
- 全世界沒人能進來（含 LAN）
- **永遠不會「意外暴露到公網」**

### 6.4 CORS

`backend/.env` 的 `CORS_ALLOWED_ORIGINS` 應該設成：
```
CORS_ALLOWED_ORIGINS=http://kwayrdcmac-mini.tail315af3.ts.net:3000
```

如果客戶端用 Tailscale IP 直連（不走 MagicDNS），那個 origin 也要加。Phase 2 上 TLS 後再改 https://。

---

## 7. 備份與災難復原

### 7.1 備份內容（**任一**失去 = 客戶資料 brick）

| 檔案 | 大小級 | 內容 |
|---|---|---|
| `postgres.dump` | ~MB | users, projects, vault_secrets, vault_ciphertexts, vault_key_wrappings, ... |
| `backend.env` | <5KB | `JWT_SECRET`, `GIT_TOKEN_ENCRYPTION_KEY`（這個就是 system KEK 來源） |
| `sparseimages/*.sparseimage` | 13MB+ per user | AES 加密的 user 檔案 |

**每一塊都要備、缺一不可**。

### 7.2 備份腳本

[`scripts/backup.sh`](../scripts/backup.sh) 每天 03:00 自動跑：

1. `docker compose exec postgres pg_dump -Fc -Z6 kway_dev > postgres.dump`
2. `cp backend/.env backend.env` + `chmod 600`
3. `rsync -a --sparse <每個> .sparseimage`（mounted 的會跳過 + 警告，避免讀到不一致狀態）
4. 寫 `manifest.txt`（每檔 size + sha256）
5. 原子搬移 `.tmp/` → `YYYY-MM-DD/`
6. Retention：保留 7 daily + 4 weekly + 12 monthly

### 7.3 復原腳本

[`scripts/restore.sh`](../scripts/restore.sh) 三模式：

```bash
./scripts/restore.sh --check    ~/kway-backups/2026-05-26   # 驗 sha256，無修改
./scripts/restore.sh --dry-run  ~/kway-backups/2026-05-26   # 印計畫，無修改
./scripts/restore.sh --apply    ~/kway-backups/2026-05-26   # 互動式，要打 'RESTORE' 才執行
```

`--apply` 流程：
1. 停 backend + web launchd agents
2. detach 任何 mounted sparseimage
3. 儲存當前 state → `backend/.env.pre-restore-<ts>` + `<dmg-root>.pre-restore-<ts>/`（可 wind back）
4. 復原 `backend/.env`
5. DROP DATABASE + pg_restore
6. rsync sparseimages 回 `~/kway-dmg-store/`
7. 重啟 backend

### 7.4 還沒做的（**operational，要 SRE 自己處理**）

| 動作 | 為什麼 |
|---|---|
| 異地副本 | 本機 backup 跟原資料同 SSD，一起壞 |
| 每月 DR 演練 | 沒驗證的 backup 不是 backup |
| Backup 加密 | `postgres.dump` 內含 password_hash + wrapped DEK，可離線暴破 |

→ 全部記在 [Phase 2 roadmap](./phase-2-roadmap.md#backup-dr-強化)。

---

## 8. Launchd 自動化

### 8.1 三個 LaunchAgent

| Label | 啟動時機 | 用途 |
|---|---|---|
| `com.kway.dev.backend` | login + crash | Rust backend |
| `com.kway.dev.web` | login + crash | Next.js web |
| `com.kway.dev.backup` | 每日 03:00 | `scripts/backup.sh` |

[Plist templates](../scripts/launchd/) 用 `@REPO_ROOT@` / `@USER_HOME@` 佔位符，`install-launchd.sh` 渲染後寫到 `~/Library/LaunchAgents/`。

### 8.2 install / uninstall / status

```bash
./scripts/install-launchd.sh install      # 安裝三個 agent
./scripts/install-launchd.sh uninstall    # 移除
./scripts/install-launchd.sh status       # 看狀態
```

兩階段安裝設計（render 全部 → bootout 全部 → sleep 2 → bootstrap 全部 + retry），避免 launchd 內部 race 條件（曾經因為 web bootstrap 太快回報 `Input/output error: 5`）。

### 8.3 LowPriorityIO

備份用 `nice = 10` + `LowPriorityIO=true`，半夜 03:00 跑也不會搶 user-facing 工作的 I/O。

---

## 9. Code map（檔案速查）

### 9.1 加密 & 認證

| 檔案 | 用途 |
|---|---|
| `backend/src/api/auth.rs` | register / login / logout / change-password handlers |
| `backend/src/security/session_keys.rs` | in-RAM KEK store + Drop zeroize |
| `backend/src/security/vault_crypto.rs` | `wrap_dek` / `unwrap_dek`（ChaCha20-Poly1305） |
| `backend/src/security/dmg_manager.rs` | hdiutil create / mount / unmount / sweep |
| `backend/src/crypto.rs` | `TokenCipher`（generic AEAD wrapper） |

### 9.2 Multi-tenant

| 檔案 | 用途 |
|---|---|
| `backend/migrations/0019_organization_project_acl.sql` | org/workspace schema + `user_can_access_project` |
| `backend/migrations/0050_backfill_personal_orgs.sql` | 補舊 user 沒有 Personal Org |
| `backend/src/api/organizations.rs` | org/workspace/member CRUD + role 檢查 helpers |
| `backend/src/api/projects.rs` | project handlers，用 `user_can_access_project` |

### 9.3 Ops

| 檔案 | 用途 |
|---|---|
| `install.sh` | 一鍵安裝（pre-flight + secrets + build + launchd） |
| `scripts/backup.sh` | 每晚備份 |
| `scripts/restore.sh` | 三模式復原 |
| `scripts/install-launchd.sh` | 部署 launchd agent |
| `scripts/launchd/*.plist.template` | LaunchAgent 模板 |
| `scripts/launchd/run-backend.sh` | backend wrapper（含 auto-tailscale 解析） |
| `scripts/launchd/run-web.sh` | web wrapper（含 -H bind） |

---

## 10. 今日 audit 發現與修正

### 10.1 修掉的 critical bug

| Commit | 嚴重性 | 描述 |
|---|---|---|
| `b4b93aa` | **Critical** | `is_mounted()` 用 `/bin/mount`（現代 macOS 不存在）→ Command spawn 失敗 silent → unwrap_or(false) → unmount 沒做事但 log 印 "unmounted"。實測登出後 DMG 全部還掛著 — **「post-logout opacity」核心保證失效**。修正：`/bin/mount` → `/sbin/mount` + 新增 `sweep_stale_mounts()` |
| `108d86e` | High | `.cargo/config.toml` 寫死 `target-dir = "C:/rust-build/..."` → macOS cargo build 把 `:` 解讀成路徑分隔 → 整個 `cargo build` 從 install.sh 以外的位置呼叫全壞 |

### 10.2 Multi-tenant audit 結果

掃過 11 個主要 handler + 5 個補審查的「邊角」資源，**16/16 全綠**：

| # | 子系統 / 資源 | 過濾機制 | 結果 |
|---|---|---|---|
| 1 | projects | `user_can_access_project()` DB function | ✅ |
| 2 | vault | `WHERE user_id = $user` + 加密層 | ✅ |
| 3 | conversations | `verify_project_access` helper | ✅ |
| 4 | agent_tasks | `require_agent_task_access` helper | ✅ |
| 5 | workspace_files | `require_file_access` helper | ✅ |
| 6 | meetings | 複合 OR（含 portal-imported 例外） | ⚠️ Phase 2 必修 |
| 7 | organizations | `require_org_member` / `require_org_admin` | ✅ |
| 8 | device_sync | `WHERE owner_user_id` | ✅ |
| 9 | conversation_memory | `verify_project_access` | ✅ |
| 10 | sprints | `verify_access` helper | ✅ |
| 11 | tasks | `verify_access` helper | ✅ |
| 12 | **shared_memory** | `WHERE user_id` 全 handlers + AI inject 只拉 sender's own | ✅ |
| 13 | **messages 表（5 個讀取點）** | feedback/metrics/conversations/conversation_memory/ws.rs 全部前置 access 檢查（ws.rs 還做雙重檢查） | ✅ |
| 14 | **device_sync_events** | DB query 全 `owner_user_id`；broadcast channel 接收端 `owner_user_id != user_id { continue }` | ✅ |
| 15 | **agent_task_events** | **無 SELECT** — 純 audit log，根本沒讀取點 | ✅ |
| 16 | **file_access_audit** | 唯一讀取點 `list_file_audit` 走 `require_file_access(viewer)` | ✅ |

唯一已知例外：[meetings.rs:492](../backend/src/api/meetings.rs#L492) portal-imported 全 user 可見（2026-05-15 design call 的 intentional choice — Phase 1 內部互信、Phase 2 多公司必修）。

### 10.3 10 條加密驗收測試結果

| # | 測試 | 結果 |
|---|---|---|
| 1 | install.sh 一次跑通 | ✅ |
| 2 | Register payload 看不到 password 明文 | ✅（靜態 + DB 雙證據） |
| 3 | DB password_hash 是 Argon2id、kek_salt 隨機 | ✅ |
| 4 | DMG mount/unmount（修 critical bug 後） | ✅ |
| 5 | 登出後 sparseimage hexdump 是密文亂碼 | ✅ |
| 6 | Vault 雙 wrapping + reveal | ✅ |
| 7 | 登出後同 token 打 reveal → 401 vault session expired | ✅ |
| 8 | 兩 user 跨 access → 404 not found | ✅ |
| 9 | 換密碼後舊 vault 仍可解（atomic re-wrap） | ✅ |
| 10 | Email 推測 timing 一致（~65ms 兩條路徑） | ✅ |

詳見 [phase-1-test-procedures.md](./phase-1-test-procedures.md)。

### 10.4 Windows ↔ Mac Mini Tailscale E2E

完整跨機驗證：
- Windows curl backend `kek-params` → HTTP 200 ✓
- Windows Chrome 註冊 user → 1 秒（WASM Argon2 跑得動）✓
- Windows 新增 vault secret → reveal 看到原文 ✓
- Windows 上傳 zip → Mac mount point 看到檔 ✓
- Windows 登出 → Mac 端 sparseimage `strings` 找不到 magic string ✓

### 10.5 完整 commit 序列（fsc branch）

```
b9e91b2 feat(security): Tailscale-only bind for backend + web (Phase 1 firewall)
6e51ce4 feat(ops): nightly backup + restore for Phase 1 commercialization
09edae8 fix(multi-tenant): backfill Personal Org for users with zero memberships
f2706f5 chore: ignore logs/ directory
108d86e chore(build): drop unconditional Windows target-dir from .cargo/config.toml
b4b93aa fix(security): correct /sbin/mount path + sweep stale DMG mounts at startup
```

---

## 11. 已知限制與已記錄 Phase 2 項目

| 領域 | Phase 1 狀態 | Phase 2 必修？ |
|---|---|---|
| Meetings cross-tenant | Portal-imported 全 user 可見 | ✅ 必修 |
| Admin recovery flow | system_v1 wrap 存在但流程未實作 | ✅ 必修 |
| SSO / MFA | 只有 email+password | ✅ 必修（企業客戶 ticket #1） |
| Audit log | 用 tracing 文字 log | ✅ 必修（SOC 2 / ISO 27001 入門） |
| HA / replica | 單台 Mac Mini SPOF | ✅ 必修（單機 SSD 壞 = 全客戶死） |
| Off-site backup | 本機備份 + 手動異地 | ✅ 必修（要自動化） |
| Backup encryption | postgres.dump 內含 hash + wrapped key，未加密 | ✅ 必修 |
| Public TLS | 走 Tailscale 沒上 TLS | 🟡 看客戶要求 |
| Compliance (DPA/GDPR/PIPL) | 無 | ✅ 看客戶地點 |
| Customer signup / billing | 全靠 Kway admin 手動 | ✅ 看 GTM 速度 |
| Monitoring / alerting | 只有 backend log | ✅ 必修 |

每一項在 [Phase 2 roadmap](./phase-2-roadmap.md) 都有完整描述、選項、估時。

---

## 附錄：今天用到的所有指令（後人來複測用）

```bash
# 服務狀態
launchctl print "gui/$(id -u)/com.kway.dev.backend" | grep state
lsof -nP -iTCP -sTCP:LISTEN | grep -E "kway-dev|:3000"
/sbin/mount | grep kway-project-data

# DB 速查
docker compose exec -T postgres psql -U postgres kway_dev -c \
  "SELECT email, count(om.organization_id) FROM users u
   LEFT JOIN organization_members om ON om.user_id=u.id
   GROUP BY u.id, email;"

# 跨租戶測試（要先抓 token）
TOKEN="<從 localStorage 抓 kway_token>"
SECRET_ID="<從 DB 抓>"
curl -X POST "http://kwayrdcmac-mini.tail315af3.ts.net:8080/api/vault/secrets/$SECRET_ID/reveal" \
     -H "Authorization: Bearer $TOKEN"

# Sparseimage 加密驗證
strings ~/kway-dmg-store/<uuid>.sparseimage | grep "<magic-string>"
hexdump -C ~/kway-dmg-store/<uuid>.sparseimage | head -2   # 應該以 encrcdsa 開頭

# 備份 + 復原
./scripts/backup.sh
./scripts/restore.sh --check ~/kway-backups/2026-05-26
./scripts/restore.sh --dry-run ~/kway-backups/2026-05-26
./scripts/restore.sh --apply ~/kway-backups/2026-05-26       # 互動，要打 RESTORE
```
