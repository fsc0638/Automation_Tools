# Phase 1 測試流程（可重跑驗收清單）

> **用途**：每次重大改動後跑一次，確認 Phase 1 加密 + 多租戶 + 網路隔離仍然成立
> **預估時間**：完整跑一遍 60-90 分鐘
> **需要的人**：1 人在 Mac Mini、1 人（或同一人換到）在 Windows
> **配套文件**：[Phase 1 實作報告](./phase-1-implementation.md)、[Phase 2 Roadmap](./phase-2-roadmap.md)

---

## 環境準備（5 分鐘）

### 終端機分配

開三個終端機，每個專用一件事：

| 終端機 | 用途 |
|---|---|
| **T1** | 跑指令（在 Mac Mini 上） |
| **T2** | 看 backend log：`tail -f logs/backend.out.log` |
| **T3** | DB 查詢：`docker compose exec postgres psql -U postgres kway_dev`（測試開始時再開） |

### 瀏覽器準備

- **Mac**：Chrome 一般視窗 + 無痕視窗（兩個 user 用）
- **Windows**：Chrome（測試 Tailscale E2E 用，在測試 #15 用到）

### 測試檔案

在桌面準備一個 zip 內含獨特字串：
```bash
echo "ENCRYPTION_PHASE1_TEST_MAGIC_STRING" > /tmp/test.txt
(cd /tmp && zip -q ~/Desktop/encryption_test.zip test.txt)
```
這個 magic string 在測試 5 / 8 用來「在 sparseimage 找它找不到」。

---

## 通關清單（你跑的時候就在這裡打勾）

| # | 測試 | 通過 | 備註 |
|---|---|---|---|
| 1 | install.sh + 服務就緒 | ☐ | |
| 2 | Register 不送明文密碼 | ☐ | |
| 3 | DB 只有 hash + ciphertext | ☐ | |
| 4 | DMG mount + unmount | ☐ | |
| 5 | 上傳檔案 + 登出後 sparseimage 是亂碼 | ☐ | |
| 6 | Vault 雙 wrapping + reveal | ☐ | |
| 7 | 登出後 server session_keys 歸零 | ☐ | |
| 8 | 兩 user 完全隔離 | ☐ | |
| 9 | 換密碼 DEK rewrap | ☐ | |
| 10 | Timing 一致防 email 推測 | ☐ | |
| 11 | Multi-tenant audit | ☐ | DB function 檢查 |
| 12 | Backup 跑 + 驗 manifest | ☐ | |
| 13 | Restore --check + --dry-run | ☐ | |
| 14 | Tailscale-only firewall | ☐ | |
| 15 | Windows 透過 Tailscale E2E | ☐ | 需要 Windows 機器 |

全綠 = Phase 1 通過。任一條失敗 = 不要往 Phase 2 前進。

---

## 測試 1 — install.sh + 服務就緒

### 目的
確認所有服務（backend、web、postgres）都在跑、設定檔還在、endpoint 還回得了。

### T1 指令
```bash
cd ~/kway/Automation_Tools

# Pre-flight 模式（不會動任何東西）
./install.sh --check
```

**通過條件**：
- ✓ macOS / Homebrew / Docker 都打勾
- ⚠️ Hermes / OpenClaw 可以警告（不影響加密測試）

### T1 確認服務
```bash
echo "=== JWT_SECRET / GIT_TOKEN_ENCRYPTION_KEY 存在 ==="
grep -c '^JWT_SECRET=' backend/.env                   # 應 = 1
grep -c '^GIT_TOKEN_ENCRYPTION_KEY=' backend/.env     # 應 = 1
grep '^SERVER_HOST=' backend/.env                     # 應 = auto-tailscale

echo "=== 目錄結構 ==="
ls -ld ~/kway-project-data/users                      # 應存在
ls -ld ~/kway-dmg-store                               # 應 drwx------ (700)

echo "=== launchd agents ==="
for label in com.kway.dev.backend com.kway.dev.web com.kway.dev.backup; do
  state=$(launchctl print "gui/$(id -u)/$label" 2>&1 | grep "^\sstate" | head -1 | xargs)
  echo "  $label  →  $state"
done

echo "=== Postgres healthy ==="
docker ps --format "table {{.Names}}\t{{.Status}}" | grep postgres

echo "=== Backend / Web 監聽 ==="
lsof -nP -iTCP -sTCP:LISTEN | grep -E "kway-dev|:3000"

echo "=== kek-params endpoint (走 Tailscale URL) ==="
curl -s "http://kwayrdcmac-mini.tail315af3.ts.net:8080/api/auth/kek-params?email=test@test.com" -w "\n[HTTP %{http_code}]\n"
```

**通過條件**：
- backend.env 有 JWT_SECRET、GIT_TOKEN_ENCRYPTION_KEY、SERVER_HOST=auto-tailscale
- 三個 launchd agent 狀態正確（backend/web = running、backup = not running 也算正常，等 03:00）
- Postgres `Up (healthy)`
- backend 監聽 `100.74.166.x:8080`（**不是** `*:8080` 或 `127.0.0.1:8080`）
- web 監聽 `100.74.166.x:3000`
- HTTP 200 + JSON `{"kek_salt":"...",...}`

### ☐ 測試 1 通過

---

## 測試 2 — Register 不送明文密碼

### 目的
證明 user 註冊時，瀏覽器送給 server 的 payload 完全沒有 password 欄位。

### 從 Mac 上測（也可從 Windows 測，效果同）

1. **Chrome** 開 `http://kwayrdcmac-mini.tail315af3.ts.net:3000/register`
2. 開 DevTools (`⌘⌥I`) → **Network** 分頁
   - 上方過濾器點 **Fetch/XHR**（不要 Wasm）
   - 勾 **Preserve log** + **Disable cache**
3. 填表（**不要按建立帳號**）：
   - email: `test_alice_<隨機數字>@test.com`（避免跟舊測試衝突）
   - 顯示名稱: `Alice Test`
   - 密碼: `MySecretPassword123`
4. Network 區左上角 🚫 清空
5. **按建立帳號**

### 預期看到的 request

| 順序 | Method + URL | 結果 |
|---|---|---|
| 1 | `GET /api/auth/kek-params?email=test_alice_...` | 200, JSON 含 kek_salt + argon2_* |
| 2 | `POST /api/auth/register` | 200/201 |

點 `POST register` → 切 **Payload** 分頁 → 看 JSON：

✅ 應該長這樣：
```json
{
  "email": "test_alice_xxx@test.com",
  "display_name": "Alice Test",
  "kek_salt": "<base64 32 bytes>",
  "auth_hash": "<base64 32 bytes>",
  "user_kek": "<base64 32 bytes>"
}
```

❌ 絕對不該看到：
- `password` 欄位
- `MySecretPassword123` 任何形式

### T1 同時驗（補強證據）
```bash
# Backend 結構體就沒有 password field — 連 parser 都拒絕
grep -A8 "pub struct RegisterRequest" backend/src/api/auth.rs
```

預期看到 5 個 field（email, display_name, kek_salt, auth_hash, user_kek），**沒有** password。

### ☐ 測試 2 通過

---

## 測試 3 — DB 只有 hash + ciphertext

### T3 (psql)
```sql
-- 1. 新註冊的 user，password_hash 是 Argon2id PHC 格式
SELECT email, display_name,
       substring(password_hash, 1, 30) AS hash_prefix,
       length(password_hash) AS hash_len,
       encode(kek_salt, 'hex') AS salt_hex
FROM users
WHERE email LIKE 'test_alice_%';
```

**通過條件**：
- `hash_prefix` 開頭 `$argon2id$v=19$m=...`
- `hash_len` ≥ 90
- `salt_hex` 64 個 hex 字元（32 bytes）
- 完全找不到明文密碼

```sql
-- 2. 該 user 已自動拿到 DMG passphrase 的雙 wrapping
SELECT kw.object_type, kw.kek_alias, length(kw.wrapped_dek) AS bytes
FROM vault_key_wrappings kw
JOIN users u ON u.id::text = kw.object_id::text
WHERE u.email LIKE 'test_alice_%'
ORDER BY kw.kek_alias;
```

**通過條件**：
- 兩筆 row
- `object_type = 'user_dmg_key'`
- `kek_alias` 一筆 `system_v1`、一筆 `user:<uuid>`
- 每筆 `bytes = 96`

### ☐ 測試 3 通過

---

## 測試 4 — DMG mount + unmount

### 4a. 登入時 DMG mount

新註冊的 user 應該已經登入 → DMG 已 mount。

### T1
```bash
echo "=== Mount status ==="
/sbin/mount | grep "kway-project-data"

echo "=== sparseimage 檔案存在 ==="
ls -la ~/kway-dmg-store/*.sparseimage | tail -3
```

**通過條件**：
- Mount 列表有 `/dev/diskNsM on /Users/.../kway-project-data/users/<uuid>` 的 entry
- `~/kway-dmg-store/<uuid>.sparseimage` 存在（約 13MB）

### 4b. 登出 → DMG unmount

在瀏覽器點登出。等 3 秒。

### T1
```bash
/sbin/mount | grep "kway-project-data" || echo "✅ 全部 unmount"
hdiutil info | grep "<uuid>" || echo "✅ 已從 image list 移除"
ls -la ~/kway-dmg-store/<uuid>.sparseimage   # 還在
```

**通過條件**：
- Mount 列表沒有該 user 的 entry
- hdiutil info 沒有該 sparseimage
- sparseimage 檔案**還在**（只是 detach 了）

### ☐ 測試 4 通過

---

## 測試 5 — 上傳檔案 + 登出後 sparseimage 是亂碼

> ⚠️ **這是最重要的測試**，證明「登出 = Mac Mini 自己也讀不到」。

### 5a. 上傳

1. 重新登入 alice
2. 進「工作管理」→ 新增工作區
3. 種類：**程式碼工作區**
4. 來源：**上傳**
5. 選 `~/Desktop/encryption_test.zip`
6. 建立

### T1 確認檔案到 DMG mount
```bash
ALICE_UUID=$(docker compose exec -T postgres psql -U postgres kway_dev -t -c \
  "SELECT id FROM users WHERE email LIKE 'test_alice_%' ORDER BY created_at DESC LIMIT 1;" \
  | tr -d ' \n' | tail -c 36)

find ~/kway-project-data/users/$ALICE_UUID/ -name "test.txt"
cat ~/kway-project-data/users/$ALICE_UUID/upload-*/test.txt
```

**通過條件**：
- 找到 `test.txt`
- 讀檔顯示 `ENCRYPTION_PHASE1_TEST_MAGIC_STRING`

### 5b. 登出 → 攻擊 sparseimage

瀏覽器點登出。等 5 秒。

### T1（致命一擊）
```bash
SPARSE=~/kway-dmg-store/${ALICE_UUID}.sparseimage

echo "=== Mount 確實 unmount ==="
/sbin/mount | grep "$ALICE_UUID" || echo "✅"

echo "=== Mount point 是空的 ==="
ls -la ~/kway-project-data/users/$ALICE_UUID/   # 應該是空目錄

echo "=== strings 找 magic string ==="
strings "$SPARSE" | grep "ENCRYPTION_PHASE1_TEST_MAGIC_STRING" \
  && echo "❌ 找到明文 — 加密失敗" \
  || echo "✅ strings 找不到"

echo "=== grep -a 二進位搜 ==="
LC_ALL=C grep -a "ENCRYPTION_PHASE1_TEST_MAGIC_STRING" "$SPARSE" \
  && echo "❌ 找到" \
  || echo "✅ 找不到"

echo "=== 連檔名也不洩漏 ==="
strings "$SPARSE" | grep "test.txt" \
  && echo "❌ 檔名洩漏" \
  || echo "✅ 連檔名都不洩漏"

echo "=== Sparseimage header 應該是 encrcdsa (AES-CDSA) ==="
hexdump -C "$SPARSE" | head -1
# 預期看到 "65 6e 63 72 63 64 73 61  ... |encrcdsa........|"
```

**通過條件**：
- 全部 ✅
- header 看到 `encrcdsa`

### ☐ 測試 5 通過 ← 加密核心保證

---

## 測試 6 — Vault 雙 wrapping + reveal

### 6a. 在瀏覽器加 secret

重新登入 alice → 左側 sidebar → **Vault** → 新增：
- label: `Test Phase1 PAT`
- value: `phase1_secret_token_XYZ`
- type: `api_key`

### T3 (psql)
```sql
-- ciphertext 不含明文
SELECT vs.label,
       length(vc.ciphertext) AS ct_bytes,
       (vc.ciphertext::text LIKE '%phase1_secret_token%')::text AS leaked
FROM vault_secrets vs
JOIN vault_ciphertexts vc ON vc.object_id = vs.id
JOIN users u ON u.id = vs.user_id
WHERE u.email LIKE 'test_alice_%'
  AND vs.label = 'Test Phase1 PAT';

-- 雙 wrapping
SELECT vs.label, kw.kek_alias, length(kw.wrapped_dek) AS bytes
FROM vault_secrets vs
JOIN vault_key_wrappings kw ON kw.object_id = vs.id
JOIN users u ON u.id = vs.user_id
WHERE u.email LIKE 'test_alice_%' AND vs.label = 'Test Phase1 PAT'
ORDER BY kw.kek_alias;

-- 全表搜尋確認沒明文
SELECT count(*) FROM vault_ciphertexts WHERE ciphertext::text LIKE '%phase1_secret_token%';
```

**通過條件**：
- ciphertext 長度合理（~40 bytes）、`leaked = false`
- 兩筆 wrapping：`system_v1` + `user:<uuid>` 各 96 bytes
- 全表搜尋 count = 0

### 6b. UI Reveal

瀏覽器 vault 頁 → 該筆 secret → 點眼睛圖示 reveal。

**通過條件**：看到 `phase1_secret_token_XYZ`（含倒數提示）

### ☐ 測試 6 通過

---

## 測試 7 — 登出後 server session_keys 歸零

### 7a. 抓 token（趁登入著）

瀏覽器 DevTools → **Application** → **Local storage** → `http://kwayrdcmac-mini.tail315af3.ts.net:3000`

找 `kway_token` → 整串複製。

### 7b. 拿 token 打 reveal API（應該成功）

### T1
```bash
TOKEN="eyJ0eXAiOiJKV1...<貼上整串>"
SECRET_ID=$(docker compose exec -T postgres psql -U postgres kway_dev -t -c \
  "SELECT vs.id FROM vault_secrets vs JOIN users u ON u.id = vs.user_id
   WHERE u.email LIKE 'test_alice_%' AND vs.label = 'Test Phase1 PAT';" \
  | tr -d ' \n' | tail -c 36)

curl -s -X POST "http://kwayrdcmac-mini.tail315af3.ts.net:8080/api/vault/secrets/$SECRET_ID/reveal" \
     -H "Authorization: Bearer $TOKEN" -w "\n[HTTP %{http_code}]\n"
```

**通過條件**：HTTP 200 + `{"secret_value":"phase1_secret_token_XYZ"}`

### 7c. 登出 → 同 token 再打一次（應該失敗）

瀏覽器登出。等 3 秒。

```bash
# 同樣 token、同樣 SECRET_ID 再打一次
curl -s -X POST "http://kwayrdcmac-mini.tail315af3.ts.net:8080/api/vault/secrets/$SECRET_ID/reveal" \
     -H "Authorization: Bearer $TOKEN" -w "\n[HTTP %{http_code}]\n"
```

**通過條件**：
- HTTP **401**
- Body: `{"error":"Vault session expired — please log in again to access vault secrets"}`
- 證明 JWT 沒過期、但 session_keys[user_id] 已歸零

### ☐ 測試 7 通過

---

## 測試 8 — 兩 user 完全隔離

### 8a. 用無痕視窗註冊 user2

Mac Chrome → ⌘⇧N 開無痕 → register
- email: `test_bob_<隨機>@test.com`
- 密碼: `BobPassword456`
- 顯示名稱: `Bob Test`

進去後加一筆 vault：
- label: `Bob's PAT`
- value: `bob_token_isolation_test`

也建一個 upload 專案，傳檔（用桌面同一個 zip 也可）。

### 8b. 用 alice token 試讀 bob 的東西

### T3 找 bob 的 project id
```sql
SELECT p.id FROM projects p
JOIN users u ON u.id = p.user_id
WHERE u.email LIKE 'test_bob_%'
ORDER BY p.created_at DESC LIMIT 1;
```

### T1 用 alice 還登入著的 token 試讀 bob 的 project
```bash
# 先重新登入 alice 抓新 token（前一個已過期）
# ... 從 DevTools 抓新的 kway_token ...
ALICE_TOKEN="<貼上>"

BOB_PROJECT_ID="<從 T3 抓>"
curl -s "http://kwayrdcmac-mini.tail315af3.ts.net:8080/api/projects/$BOB_PROJECT_ID" \
     -H "Authorization: Bearer $ALICE_TOKEN" -w "\n[HTTP %{http_code}]\n"
```

**通過條件**：
- HTTP **404** "Project not found"（不是 403 — 連存在性都不洩漏）

### 8c. 檔案系統層隔離

### T1
```bash
ls ~/kway-project-data/users/      # 看到至少兩個 uuid 目錄
ls ~/kway-dmg-store/                # 看到至少兩個 sparseimage

# 確認 alice 看不到 bob 檔案、bob 看不到 alice 檔案
# （因為兩個 mount 都是各自加密、各自掛載點）
```

**通過條件**：每人各自 sparseimage、各自掛載點。

### ☐ 測試 8 通過

---

## 測試 9 — 換密碼後舊 vault 仍可解（DEK rewrap）

### 9a. 變更密碼

用 alice 重新登入 → vault 頁 → **變更密碼**：
- 目前密碼: `MySecretPassword123`
- 新密碼: `RotatedPassword789`

成功後**不要登出**。

### 9b. 原本的 secret 仍可 reveal

vault 頁 → `Test Phase1 PAT` → 點 reveal → 應該還是看到 `phase1_secret_token_XYZ`

**為什麼這證明 atomic rewrap 成功**：
- 換密碼那一刻：server 用舊 user_kek unwrap DEK、用新 user_kek 重 wrap DEK
- 你現在 session 用新 user_kek，能解開新 wrap → 拿到 DEK → 解開 ciphertext
- 如果 rewrap 沒做 / 半路斷掉，這個 reveal 會 500 失敗

### 9c. 登出 → 用新密碼重登 → 再 reveal 一次

登出 → 用 `RotatedPassword789` 登入 → reveal → 仍應看到原文。

**通過條件**：兩次 reveal 都成功。

### 9d. DB 驗證 password_hash 變了
```sql
SELECT email, substring(password_hash, 30, 30) AS hash_middle
FROM users WHERE email LIKE 'test_alice_%';
```
hash_middle 跟測試 3 抓的 hash 中段**應該不同**（password_hash 變了）。

### ☐ 測試 9 通過

---

## 測試 10 — Timing 一致防 email 推測

### T1
```bash
ZERO_B64="AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="

echo "=== (a) 存在的 email + 錯誤 hash ==="
for i in 1 2 3; do
  T0=$(python3 -c 'import time; print(time.time())')
  HTTP=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
    "http://kwayrdcmac-mini.tail315af3.ts.net:8080/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"test_alice_$(date +%s)@test.com\",\"auth_hash\":\"$ZERO_B64\",\"user_kek\":\"$ZERO_B64\"}")
  T1=$(python3 -c 'import time; print(time.time())')
  python3 -c "print(f'  run $i: HTTP $HTTP, {($T1 - $T0)*1000:.0f}ms')"
done

echo "=== (b) 不存在的 email + 錯誤 hash ==="
for i in 1 2 3; do
  T0=$(python3 -c 'import time; print(time.time())')
  HTTP=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
    "http://kwayrdcmac-mini.tail315af3.ts.net:8080/api/auth/login" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"nobody-$i@nowhere.com\",\"auth_hash\":\"$ZERO_B64\",\"user_kek\":\"$ZERO_B64\"}")
  T1=$(python3 -c 'import time; print(time.time())')
  python3 -c "print(f'  run $i: HTTP $HTTP, {($T1 - $T0)*1000:.0f}ms')"
done
```

**通過條件**：
- 全部 HTTP 401
- (a) 跟 (b) 的平均時間**差距 < 20ms**（基本上看不出來）

### ☐ 測試 10 通過

---

## 測試 11 — Multi-tenant audit

### 11a. user_can_access_project DB function 完整

### T3
```sql
\df+ user_can_access_project
```

**通過條件**：function source 含四條 OR：
- `p.user_id = p_user_id` (direct owner)
- `pa.role` (project ACL)
- `om.role` (org member)
- `wm.role` (workspace member)

### 11b. 每個 user 都有 Personal Org

### T3
```sql
SELECT u.email, count(om.organization_id) AS orgs
FROM users u
LEFT JOIN organization_members om ON om.user_id = u.id
GROUP BY u.email
HAVING count(om.organization_id) = 0;
```

**通過條件**：**空集合**（沒有 user 是 0 orgs）

### 11c. 確認 handler 都有授權檢查

### T1
```bash
echo "=== 主要 handler 都用 user_can_access_project 或 require_*_access ==="
for f in projects vault meetings conversations agent_tasks workspace_files \
         conversation_memory device_sync organizations sprints; do
  fp="backend/src/api/${f}.rs"
  [[ -f "$fp" ]] || continue
  checks=$(grep -cE "require_|verify_|user_can_access|WHERE.*user_id = \\\$|owner_user_id = \\\$" "$fp")
  echo "  $fp:  $checks auth checks"
done
```

**通過條件**：每個檔案 ≥ 1（多數應該 ≥ 3）

### ☐ 測試 11 通過

---

## 測試 12 — Backup 跑 + 驗 manifest

### T1
```bash
echo "=== 手動跑一次 backup ==="
scripts/backup.sh 2>&1 | tail -10

echo "=== 看今天 backup 目錄 ==="
TODAY=$(date +%Y-%m-%d)
ls -la ~/kway-backups/$TODAY/

echo "=== Manifest 內容 ==="
cat ~/kway-backups/$TODAY/manifest.txt
```

**通過條件**：
- 看到 `backup complete: /Users/.../kway-backups/YYYY-MM-DD`
- 目錄內：`postgres.dump`、`backend.env`、`sparseimages/`、`manifest.txt`、`backup.log`
- Manifest 列出每個檔案的 size + sha256

### 12b. 排程確認
```bash
launchctl print "gui/$(id -u)/com.kway.dev.backup" 2>&1 | \
  grep -E "state|StartCalendarInterval|Hour|Minute" | head -10
```

**通過條件**：有 `state` + `StartCalendarInterval` 含 `Hour = 3` `Minute = 0`

### ☐ 測試 12 通過

---

## 測試 13 — Restore --check + --dry-run

> ⚠️ **不要跑 `--apply`**，那會 drop DB。要驗 apply 在乾淨環境另外跑。

### T1
```bash
echo "=== --check：驗 sha256 ==="
scripts/restore.sh --check ~/kway-backups/$(date +%Y-%m-%d) 2>&1 | tail -5

echo "=== --dry-run：印計畫 ==="
scripts/restore.sh --dry-run ~/kway-backups/$(date +%Y-%m-%d) 2>&1 | tail -10
```

**通過條件**：
- `--check`：`manifest OK — all files present and matching sha256` + `check complete`
- `--dry-run`：印出完整 plan、`dry-run complete`

### ☐ 測試 13 通過

---

## 測試 14 — Tailscale-only firewall

### T1
```bash
echo "=== 1. ✅ Tailscale URL 應通 ==="
curl -s --max-time 3 -o /dev/null -w "HTTP %{http_code}\n" \
     "http://kwayrdcmac-mini.tail315af3.ts.net:8080/api/auth/kek-params?email=test@test.com"

echo "=== 2. ❌ localhost 應斷 ==="
curl -s --max-time 3 -o /dev/null -w "HTTP %{http_code}\n" \
     "http://localhost:8080/api/auth/kek-params?email=test@test.com"

echo "=== 3. ❌ LAN IP 應斷（找你自己的 LAN IP）==="
LAN_IP=$(ifconfig | grep "inet 192.168\|inet 172\." | grep -v "127.0.0" | head -1 | awk '{print $2}')
echo "  LAN IP: $LAN_IP"
curl -s --max-time 3 -o /dev/null -w "HTTP %{http_code}\n" \
     "http://${LAN_IP}:8080/api/auth/kek-params?email=test@test.com"

echo "=== 4. Backend / Web 確實只 bind Tailscale IP ==="
lsof -nP -iTCP -sTCP:LISTEN | grep -E "kway-dev|:3000"
```

**通過條件**：
1. HTTP 200
2. HTTP 000（connection refused）
3. HTTP 000（connection refused）
4. 看到 `100.74.166.x:8080` 和 `100.74.166.x:3000`（**不是** `*:8080`）

### ☐ 測試 14 通過

---

## 測試 15 — Windows 透過 Tailscale E2E

> 這是「**外部 user 真的能用**」的最終驗證。需要 Windows 機器。

### 15a. Windows 端準備

1. https://tailscale.com/download/windows 下載安裝
2. 用同個 Tailscale 帳號登入（跟 Mac Mini 同 tailnet）
3. 系統匣 Tailscale 圖示變藍

### 15b. PowerShell 連線驗證

```powershell
tailscale status                           # 應看到 kwayrdcmac-mini
tailscale ping kwayrdcmac-mini             # 應 pong via direct (或 DERP)

# 注意用 curl.exe 不是 curl (alias)
curl.exe -s -w "`nHTTP %{http_code}`n" `
  "http://kwayrdcmac-mini.tail315af3.ts.net:8080/api/auth/kek-params?email=test@test.com"
```

**通過條件**：HTTP 200 + JSON

### 15c. Windows Chrome E2E

1. 開 `http://kwayrdcmac-mini.tail315af3.ts.net:3000`
2. 註冊 user `test_windows_<隨機>@test.com`
3. 預期 1-2 秒跳轉（Argon2 WASM）
4. 加 vault secret + 上傳 zip
5. 登出

### 15d. Mac 端驗檔案進 DMG 且登出後 sparseimage 是亂碼

### T1
```bash
WIN_UUID=$(docker compose exec -T postgres psql -U postgres kway_dev -t -c \
  "SELECT id FROM users WHERE email LIKE 'test_windows_%' ORDER BY created_at DESC LIMIT 1;" \
  | tr -d ' \n' | tail -c 36)

echo "=== 登出後 sparseimage 找 magic（應找不到） ==="
strings ~/kway-dmg-store/${WIN_UUID}.sparseimage | \
  grep "ENCRYPTION_PHASE1_TEST_MAGIC_STRING" \
  && echo "❌ 找到" || echo "✅ 加密成功"
```

**通過條件**：✅ 加密成功

### ☐ 測試 15 通過

---

## 收尾：清理測試資料

跑完所有測試後，刪掉測試用 user（保留正式 user）：

### T3
```sql
-- 看哪些是測試 user
SELECT email FROM users WHERE email LIKE 'test_%';

-- 確認後刪除（小心！）
DELETE FROM users WHERE email LIKE 'test_%';
-- CASCADE 會連帶刪 vault / project / org_members / DMG wrappings
```

### T1
```bash
# 對應的 sparseimage 也清掉（user 沒了就成孤兒）
docker compose exec -T postgres psql -U postgres kway_dev -t -c \
  "SELECT id FROM users;" | tr -d ' ' | sort > /tmp/active_uuids.txt

for img in ~/kway-dmg-store/*.sparseimage; do
  uuid=$(basename "$img" .sparseimage)
  grep -qx "$uuid" /tmp/active_uuids.txt || {
    echo "Removing orphan: $img"
    # 取消註解才會真刪
    # rm "$img"
  }
done
```

---

## 全綠後

把這個檔案 commit 並 push 一個 `tested-YYYY-MM-DD` git tag：
```bash
git tag tested-$(date +%Y-%m-%d)
git push origin tested-$(date +%Y-%m-%d)
```

這樣團隊知道哪個 commit 是「已驗證」狀態。

---

## 失敗時怎麼除錯

| 測試失敗 | 第一個看的地方 |
|---|---|
| 1 / install | `logs/backend.err.log` + `docker compose logs postgres` |
| 2 / register payload | DevTools Network 看實際 payload，貼出來 |
| 3 / DB hash | psql 確認 `_sqlx_migrations` 表有 0049 + 0050 |
| 4 / DMG mount | `tail -50 logs/backend.out.log \| grep dmg` |
| 5 / sparseimage 找到明文 | **STOP，立刻 escalate**，加密失效是 P0 |
| 6 / vault wrap | 確認 `vault_keys` 表有 `system_v1` row |
| 7 / 401 | 確認 backend 真的有重啟（log 時間戳） |
| 8 / 404 | 確認 user 真的在不同 org |
| 9 / rewrap | 看 `re_wrap_deks_and_update_password` 的 transaction log |
| 10 / timing 差太大 | 確認 missing user 有跑 dummy verify（grep `dummy-input-for-timing`） |
| 11 / audit | 看 [phase-1-implementation.md §5](./phase-1-implementation.md#5-multi-tenant-模型) |
| 12 / backup | 確認 docker postgres 在跑、backend/.env 存在 |
| 13 / restore | 確認 backup 目錄結構完整 |
| 14 / firewall | `lsof` 確認 bind 介面、`tailscale ip -4` 確認 IP |
| 15 / Windows | PowerShell `tailscale ping` 不通 = Tailscale 設定問題 |

無法判斷 → 把 T1 / T2 的完整輸出貼到 issue tracker，含時間戳。

---

## 變更歷史

| 日期 | 變更 |
|---|---|
| 2026-05-26 | 初版，涵蓋 15 條測試（Phase 1 完整） |
