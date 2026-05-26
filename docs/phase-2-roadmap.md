# Phase 2 商品化 Roadmap

> **狀態**：2026-05-26 草擬
> **前置**：[Phase 1 已完成](./phase-1-implementation.md)
> **目標**：從「Kway 內部 + 關係企業」擴展到外部商業客戶

---

## 0. 戰略前提（必讀）

### 0.1 商業模式定調

| 項目 | 決策 |
|---|---|
| **賣什麼** | 軟體訂閱 / 使用權 |
| **不賣什麼** | **不賣硬體** |
| **硬體在哪** | Kway 公司機房內 |
| **客戶怎麼用** | 瀏覽器（或 iOS app）走網路連到 Kway 機房 |
| **資料留在哪** | Kway 機房（不出機房） |

→ 這是 **私有雲 SaaS / Self-Hosted SaaS** 模式。客戶買的是「託管服務」，Kway 是 data processor（部分情況 data controller）。

### 0.2 此架構的直接影響

| 受影響項目 | 變化 |
|---|---|
| **Compliance** | 我們扛全部責任（不是客戶的責任） |
| **HA** | 一台 Mac Mini 死 = 多家客戶服務中斷 = 上新聞 |
| **Multi-tenancy** | 必須真正隔離（同台機器有多公司） |
| **網路** | 客戶不在我們 Tailnet 內，要設計新的進入路徑 |
| **支援** | 24/7 on-call、SLA、合約 |
| **法律** | DPA、隱私政策、客戶資料 export、保險 |

---

## 1. 整體優先級

依「客戶第一個 ticket 是什麼？」+「不做會出大事」排序：

| 優先級 | 項目 | 估時 | 阻擋因子 |
|---|---|---|---|
| **P0** | [HA 雙機 + 備份異地化](#1-ha-雙機--備份異地化) | 3-4 週 | 不做 → 第一次 SSD 壞掉 = 公司關門 |
| **P0** | [Compliance 基礎（DPA、隱私政策、客戶資料 export）](#3-compliance-基礎) | 2-3 週 | 不做 → 律師信、無法簽企業客戶 |
| **P0** | [客戶網路存取（不再靠 Tailscale）](#2-客戶網路存取) | 1-2 週 | 不做 → 客戶 IT 部門不准裝 Tailscale |
| **P0** | [Audit Log 結構化](#4-audit-log-結構化) | 1-2 週 | 不做 → 安全事件無法追溯、SOC 2 fail |
| **P1** | [Multi-tenant 真正隔離（meetings + 共用 portal）](#5-multi-tenant-真正隔離) | 1-2 週 | 不做 → A 公司看到 B 公司會議 |
| **P1** | [SSO + MFA](#6-sso--mfa) | 2-3 週 | 不做 → 企業客戶 ticket #1 |
| **P1** | [Admin Recovery Flow](#7-admin-recovery-flow) | 1 週 | 不做 → 第一個 user 忘密碼來電就出包 |
| **P2** | [Backup 加密 + 跨地備援](#1-ha-雙機--備份異地化) | 含在 P0 內 | |
| **P2** | [Monitoring + Alerting](#8-monitoring--alerting) | 2 週 | 不做 → 服務掛了我們最後知道 |
| **P2** | [客戶 onboarding / billing](#9-客戶-onboarding--billing) | 視 GTM 速度 | |
| **P3** | [Meeting 視窗遮蔽（portal cross-tenant）](#5-multi-tenant-真正隔離) | 1 週 | 含在 P1 内 |
| **P3** | [TLS / 自有網域](#2-客戶網路存取) | 含在 P0 內 | |

---

## 1. HA 雙機 + 備份異地化

### 1.1 問題

Mac Mini SSD MTBF 約 5 年。若一台機器服務 10 家客戶、每家 10 員工 = 100 個重度使用者。SSD 故障 = **100 個人**的工作中斷 + 資料可能無法復原（如果 backup 也壞）。

更糟：Mac Mini 不是企業級設備，沒有 RAID、ECC RAM、redundant PSU。

### 1.2 方案選項

#### 方案 A：Mac Mini Master + Mac Mini Replica（推薦）

```
┌─────────────────┐       ┌─────────────────┐
│  Mac Mini A     │  ◄══► │  Mac Mini B     │
│  (Master)       │ DRBD  │  (Hot Standby)  │
│  - active       │ -like │  - read-only    │
│  - serves users │       │  - watches A    │
└────────┬────────┘       └────────┬────────┘
         │                         │
         └─────────┬───────────────┘
                   ▼
            ┌──────────────┐
            │  Failover    │
            │  Tailscale   │
            │  hostname    │
            └──────────────┘
```

**技術**：
- Postgres：streaming replication（Postgres native）→ B 為 read replica
- DMG sparseimages：每 5 分鐘 rsync（or APFS snapshot + ship snapshot diff）→ 接受小 RPO
- backend/.env：identical（同樣 secrets，git 不存）
- Tailscale：MagicDNS 指向 A，B 上線時搶 hostname（DNS failover）
- 偵測：UptimeRobot 或自建 heartbeat → 失聯 N 分鐘 → 手動或自動 fail-over

**估時**：3-4 週（streaming repl 1 週、DMG sync 1 週、failover automation 1-2 週）

#### 方案 B：單機 + 強備援（Plan B）

```
Mac Mini A
   │
   ▼
hourly backup → 異地 NAS (Tailscale 連)
                + S3 / B2 / Wasabi (off-site, encrypted)
```

- 接受 RPO 1 小時 + RTO 4 小時（手動 spin up replacement）
- 大幅省錢（不用買第二台）
- 適合：小客戶、可容忍短暫中斷

**估時**：1 週（自動 rsync 到 NAS + 加密上雲）

#### 方案 C：Kubernetes / Cloud（重寫）

放棄 Mac Mini，搬到 Linux / AWS。失去 DMG 加密（Linux 沒有 hdiutil），要重寫成 LUKS 或 file-level encryption。

**估時**：3-6 個月

### 1.3 建議路線

**Phase 2a**: 方案 B（hourly backup → NAS + 雲）— 1 週上線
**Phase 2b**: 方案 A（雙機 HA）— 拿到前 5 家客戶後做

### 1.4 Backup 強化清單

- [ ] 每小時 backup（不是每日）
- [ ] `postgres.dump` GPG 加密（用 ops-team 持有的 key）
- [ ] sparseimages rsync 到 Tailscale 上的 NAS
- [ ] 月度上雲（S3/B2，client-side encrypted）
- [ ] 每月一次 DR 演練（隨機抽一天 backup 做 `restore.sh --apply` 到測試環境）
- [ ] Backup 失敗 → 立刻 Slack 通知

---

## 2. 客戶網路存取

### 2.1 問題

Phase 1 走 Tailscale，假設每個 user 裝 Tailscale。商業客戶會 push back：
- 「我們公司 IT 不准裝外部 VPN」
- 「我筆電是公司資產不能裝任何 client」
- 「我要在手機隨時用」

### 2.2 方案選項

#### A. 標準 HTTPS over 公網 + 嚴格 ACL（推薦預設）

```
公網 ──► Cloudflare ──► Mac Mini :443 (Caddy / nginx)
         │                    │
         └─ DDoS/WAF           └─ TLS termination
                                  proxy_pass to :3000 / :8080
```

- 自有網域如 `app.kway.com`
- Let's Encrypt 自動續證（or `tailscale cert` 用 Tailscale-issued）
- Cloudflare 在前：DDoS 防護、地理封鎖、Rate limit
- Mac Mini 上 reverse proxy 收 443 → 內部 :3000 / :8080

**注意**：這破壞「只走 Tailscale」的 firewall — 必須改用 pfctl 鎖死 :8080 / :3000 LAN 介面，公網只開 :443 reverse proxy port。

#### B. Tailscale Funnel（簡單但綁定 Tailscale）

Tailscale 官方功能，免費把 tailnet 內服務暴露到公網（限制：流量上限、不能用自有網域）。
- ✅ 簡單，5 分鐘設定好
- ❌ 大客戶見到 ts.net 子網域會皺眉

#### C. 客戶 IT 設 VPN tunnel 過來

每個企業客戶建立 Site-to-Site VPN（IPSec 或 WireGuard）到 Kway 機房。
- 大企業客戶喜歡
- 我們要維運多條 VPN
- 中小客戶嫌麻煩

### 2.3 建議組合

| 客戶類型 | 預設用 |
|---|---|
| 個體戶 / 小團隊 | 公網 HTTPS（A） |
| 中小企業 | 公網 HTTPS + 可選 Tailscale Funnel |
| 大企業 | Site-to-Site VPN（C） |

### 2.4 必修清單

- [ ] 申請網域（`app.kway.com` 或類似）
- [ ] Cloudflare 帳號 + DNS
- [ ] Caddy / nginx reverse proxy on Mac Mini
- [ ] Let's Encrypt 自動續證
- [ ] pfctl rules：公網只開 :443、:8080 + :3000 從 Tailscale 內網
- [ ] CORS 允許新網域
- [ ] iOS app + Web 都改連新網域

**估時**：1-2 週

---

## 3. Compliance 基礎

### 3.1 我們會踩到的法規

| 法規 | 何時觸發 |
|---|---|
| **GDPR** | 客戶有 EU/UK 員工 |
| **CCPA** | 客戶有加州用戶 |
| **PIPL** | 客戶有中國用戶 |
| **PDPA** | 台灣個資法（一定踩到） |
| **HIPAA** | 客戶是醫療業 |
| **PCI-DSS** | 客戶存信用卡資料（不該存，但...） |
| **SOC 2** | 大企業客戶問 |
| **ISO 27001** | 國際客戶可能問 |

### 3.2 第一階段必備（before first paying customer）

| 文件 | 用途 |
|---|---|
| **Terms of Service** | 法律基礎 |
| **Privacy Policy** | 必須公開揭露收集什麼資料、為什麼、保存多久 |
| **DPA (Data Processing Agreement)** | 跟客戶簽：他們是 controller、我們是 processor |
| **Cookies Policy** | Web 用 cookie 就要 |
| **資料保留政策** | 客戶離開後幾天刪除 |
| **Sub-processor 清單** | 我們用 Cloudflare / S3 / Tailscale → 都要列出 |

→ **找律師**。不要 ChatGPT-generate。台灣有專做 SaaS 的律師事務所。

### 3.3 技術上的 Compliance feature 必修

- [ ] **資料 export endpoint**：GDPR Article 15「資料當事人有權拿到自己所有資料的副本」
  ```
  GET /api/users/me/export
  → 回傳 user 的所有資料（projects + vault + messages + ...）為 zip
  ```
- [ ] **資料刪除（被遺忘權）**：硬刪除 vs 軟刪除？vault 內容是否可救？
- [ ] **資料外洩通知**：72 小時內通知所有受影響 user（GDPR 規定）
- [ ] **未成年保護**：13 歲以下不能註冊（COPPA）
- [ ] **客戶資料隔離證明**：給客戶看「你的資料跟別人的隔離程度」報告

### 3.4 估時

| 動作 | 估時 |
|---|---|
| 律師起草文件 | 2-3 週（外包） |
| 資料 export endpoint | 3-5 天 |
| 刪除流程 | 1 週 |
| 整合測試 | 1 週 |

**總計 2-3 週**（含等律師）

---

## 4. Audit Log 結構化

### 4.1 現況

`tracing::info!` 寫文字 log 到 `logs/backend.out.log`：
```
[2026-05-26T03:33:59Z] INFO ... dmg: created new encrypted image user_id=2a9723f4-...
```
**搜尋難、不可竄改、無 retention、不能匯出給客戶**。

### 4.2 商品化要的東西

```sql
CREATE TABLE audit_log (
    id           BIGSERIAL PRIMARY KEY,
    timestamp    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    actor_user   UUID NOT NULL REFERENCES users(id),
    actor_org    UUID,
    action       TEXT NOT NULL,            -- 'vault.reveal', 'project.delete', 'user.login'
    resource_id  UUID,
    resource_type TEXT,
    result       TEXT NOT NULL,            -- 'success' / 'denied' / 'error'
    metadata     JSONB,                    -- 補充資訊
    prev_hash    BYTEA,                    -- chain to previous row's hash
    row_hash     BYTEA NOT NULL            -- SHA256(prev_hash || row_data)
);
CREATE INDEX idx_audit_actor ON audit_log(actor_user, timestamp DESC);
CREATE INDEX idx_audit_action ON audit_log(action, timestamp DESC);
```

**Hash chain** 讓 audit log tamper-evident：改動任何一筆，下一筆的 hash 對不起來。

### 4.3 必須記錄的事件

| Category | Action |
|---|---|
| **Auth** | login (success/fail), logout, register, change-password, refresh-token |
| **Vault** | secret.create, secret.reveal, secret.update, secret.delete |
| **Project** | create, delete, update, transfer, acl.add, acl.remove |
| **File** | upload, download, delete, audit_view |
| **Org/Workspace** | create, member.add, member.remove, role.change |
| **Admin** | recovery.use, system_key.rotate（高敏感） |
| **System** | startup, shutdown, migration_applied, sweep_stale_mounts |

### 4.4 客戶看得到的 API

```
GET /api/audit?action=vault.reveal&from=2026-01-01
→ 客戶 admin 可以看自己 org 內所有 user 的活動
```

Kway 內部 admin 看全部，per-org admin 只看自己 org。

### 4.5 估時

| 動作 | 估時 |
|---|---|
| Schema migration | 1 天 |
| audit emit middleware（所有 handler 一次塞） | 3-5 天 |
| Hash chain 邏輯 + verification | 2 天 |
| API endpoint + UI | 1 週 |
| Export 機制（csv / json / siem） | 3 天 |

**總計 2 週**

---

## 5. Multi-tenant 真正隔離

### 5.1 已知問題

[`backend/src/api/meetings.rs:492`](../backend/src/api/meetings.rs#L492)：
```rust
OR m.external_id IS NOT NULL  -- portal-imported visible to ALL users
```

Phase 1 OK（一個 Kway portal），Phase 2 NOT OK（多客戶各自 portal）。

### 5.2 Audit 還沒查的（要補）

從 Phase 1 audit 留下的 TODO：
- [ ] `shared_memory` — cross-org decision 是否會混
- [ ] `messages.user_id` — AI 對話內容是否會洩漏給同 org 別人
- [ ] `file_access_audit` — 自己沒 tenant 欄位
- [ ] `device_sync_events` — iOS 同步是否會撈到別 org event
- [ ] `agent_task_events` — agent 拿到的 context 是否限定 org

### 5.3 修法

**meetings**：
```sql
-- 加 organization_id 到 meetings（已有但沒嚴格使用）
-- list_meetings 改成：
WHERE m.organization_id IN (
    SELECT organization_id FROM organization_members WHERE user_id = $1
)
AND (
    [現有條件...]
)
```

**portal scraper**：每個 org 各自跑 scraper（或共用 scraper 但寫入時帶 org_id）。

### 5.4 估時

| 動作 | 估時 |
|---|---|
| Audit 剩下的 5 個 handler | 2-3 天 |
| meetings.rs 加 org scoping | 2 天 |
| Portal scraper per-org | 3-5 天 |
| 跨 tenant 攻擊測試自動化 | 3 天 |

**總計 1-2 週**

---

## 6. SSO + MFA

### 6.1 為什麼

企業客戶第一個問的問題（不誇張）。每家中型以上公司都用 Okta / Microsoft Entra / Google Workspace 統一管帳號。員工不會願意為了 Kway 另記一組密碼。

### 6.2 Client-held KEK 跟 SSO 的根本衝突

```
傳統 SSO：
  user → IdP → SAML assertion → app: "this user is alice@company.com"
  app → 發 JWT → user 進來，無密碼

我們的：
  user → 輸入密碼 → 客戶端算 Argon2 → KEK 派生
  KEK 必須要從 user input 派生才能保證 server 拿不到
```

SSO 流程裡 user **沒輸入密碼給我們**，所以無法派生 KEK。三條解：

#### 方案 A：SSO + 額外 vault 密碼（推薦）

- SSO 認證身份 → 進入 app（看 project / meeting 等正常功能 OK）
- 第一次用 vault → 跳「請設定 vault 密碼」
- 此後每次 reveal vault 都要再輸入 vault 密碼
- vault 密碼用 Argon2 派生 KEK，跟 Phase 1 一樣

**好處**：vault 仍然 client-held KEK；其他功能 SSO 順暢
**壞處**：UX 多一步

#### 方案 B：Hardware Key (WebAuthn / Yubikey)

- KEK 從 hardware key 派生（不是 password）
- 需要客戶買 Yubikey
- 最安全但成本高

#### 方案 C：IdP-derived KEK（不推薦）

- 用 SSO token 的某段內容當 input 算 KEK
- 簡單，但 IdP compromise = vault compromise（單一信任點）

### 6.3 MFA 選項

| 方法 | 難度 | 接受度 |
|---|---|---|
| **TOTP**（Google Authenticator / Authy） | 簡單 | 高，最基本 |
| **WebAuthn**（指紋 / Yubikey） | 中 | 中高，未來方向 |
| **SMS OTP** | 簡單 | 低（不安全，但有些客戶要） |
| **Email OTP** | 簡單 | 中 |

### 6.4 建議路線

| 階段 | 做 |
|---|---|
| Phase 2.0（first paying customer） | TOTP MFA |
| Phase 2.1 | SAML SSO（方案 A：SSO + vault 密碼） |
| Phase 2.2 | WebAuthn |
| Phase 2.3+ | SCIM (auto provision/deprovision) |

### 6.5 估時

| 動作 | 估時 |
|---|---|
| TOTP MFA | 1 週 |
| SAML SSO 整合（用 samael 或類似 crate） | 2-3 週 |
| 整合測試（Okta + Entra + Google） | 1 週 |

**總計 4-5 週**

---

## 7. Admin Recovery Flow

### 7.1 問題

[`vault-recovery-runbook.md`](./vault-recovery-runbook.md) 描述了用 `system_v1` wrap 來救 user vault 的概念，**但實際操作流程沒寫**。

實際場景：
- alice 忘了密碼 → 重設密碼後，alice 自己的 vault 全部變廢
- alice 離職 → 公司要拿回她的 vault 內容
- 司法介入（搜索票）→ Kway 要在不知道 user 密碼的情況下提供資料

### 7.2 設計

#### 7.2.1 「Recovery Key」生成（用戶自助）

註冊時生成一張紙：
```
Your Kway Recovery Key:
swing bring abandon clinic figure rule ...
(24 words BIP-39 mnemonic)
```
- user 自己保管（紙本 / 密碼管理員）
- 忘密碼時輸入 → 派生 recovery KEK → unwrap vault wrappings
- 加密幣圈標準做法

#### 7.2.2 Org Admin Recovery（企業用）

```
Org Admin Console:
[Recover vault for: bob@companya.com]
  → 必須兩個 admin 同時按按鈕（防單人惡意）
  → 必須輸入 reason（寫入 audit log）
  → 必須 24 小時冷卻期（user 可以打斷）
  → 用 system_v1 KEK unwrap → 匯出加密 zip 給 admin
```

#### 7.2.3 N-of-M 門檻（大企業選配）

`system_v1` KEK 用 Shamir Secret Sharing 拆 5 份，需 3 份才能組回。
- 3 個 admin 各持一份
- 1 個 admin 跑路不影響其他人能操作
- 1 個 admin 不夠惡意操作

### 7.3 必修清單

- [ ] User-facing：忘密碼流程 + recovery key download
- [ ] Admin-facing：「Recover user vault」按鈕
- [ ] Audit：所有 admin recovery 都進 audit_log
- [ ] Notification：user 被 admin recover 時必須收到 email
- [ ] 冷卻期：24 小時內 user 可取消
- [ ] 文件：legal-approved 流程文件

### 7.4 估時

| 動作 | 估時 |
|---|---|
| Recovery Key 生成 + UI | 3 天 |
| Org Admin Recovery flow | 1 週 |
| N-of-M (optional, Phase 2.1) | 1 週 |
| Audit + Notification + 冷卻期 | 3 天 |

**總計 1-2 週**

---

## 8. Monitoring + Alerting

### 8.1 缺什麼

| 監測項 | 現況 | Phase 2 |
|---|---|---|
| Backend liveness | 自己看 launchctl | 外部 ping → PagerDuty |
| HTTP error rate | log | Prometheus + Grafana + alert |
| DB connection pool 滿 | log | metric alert |
| Disk space (sparseimage 漲) | 沒人看 | alert when > 80% |
| Backup failure | log 看 | Slack 通知 |
| 異常 vault.reveal 頻率 | 沒有 | rate-based alert |
| 異常 login fail 集中 | 沒有 | brute-force detection |

### 8.2 建議組合

- **Metrics**：Prometheus（backend 已 export `/metrics`）+ Grafana Cloud
- **Logs**：Vector / Loki 把 `logs/*.log` 送上去
- **Alerts**：PagerDuty + Slack
- **Status page**：StatusPage.io（給客戶看）

### 8.3 估時

| 動作 | 估時 |
|---|---|
| Prometheus + Grafana setup | 3 天 |
| Loki + Vector | 3 天 |
| Alert rules | 3 天 |
| PagerDuty integration | 1 天 |
| Status page | 2 天 |

**總計 2 週**

---

## 9. 客戶 Onboarding / Billing

### 9.1 現況

Kway admin 手動：
- 建 organization
- 邀請 user
- 設密碼
- 給網址

### 9.2 商品化要的

- [ ] **Public signup page**：客戶自己註冊
- [ ] **Pricing page**：seat-based / org-based
- [ ] **Stripe 整合**：信用卡、月/年訂閱
- [ ] **Trial 機制**：14 天 free trial
- [ ] **Self-serve admin**：客戶自己邀員工 / 設角色
- [ ] **Invoice / 發票**：台灣電子發票
- [ ] **Subscription management**：升級/降級/取消
- [ ] **Usage limit enforcement**：seat 滿就不能再邀人

### 9.3 估時

| 動作 | 估時 |
|---|---|
| Signup + onboarding | 2 週 |
| Stripe + subscription | 2-3 週 |
| 台灣電子發票 | 1 週 |
| Admin console | 2 週 |

**總計 6-8 週**

→ 看 GTM 速度。如果一開始客戶都靠 sales call 簽，可以延後。

---

## 10. 其他雜項

### 10.1 文件

- [ ] **API docs** (OpenAPI / Swagger)
- [ ] **End-user 操作手冊**（中文 + 英文）
- [ ] **Admin 操作手冊**
- [ ] **Security white paper**（給企業客戶 security team 看）
- [ ] **架構公開介紹文**（給技術社群看，吸 talent / 客戶）

### 10.2 i18n

- 現在只有繁中。商品化前至少加：
  - 簡中
  - 英文
- 看市場決定要不要加日文 / 韓文

### 10.3 行動端

- iOS app 還沒走完 client-held KEK 整套（需要驗證）
- Android app 沒做

### 10.4 性能

- 單台 Mac Mini 能服務多少人？沒實測過。
- DMG 數量上限？hdiutil 同時 mount 數有上限。
- Postgres 在 Docker 內、Mac Mini 16GB RAM 能撐多少 connections？

---

## 11. 時間軸（建議）

```
Month 1
├── Week 1-2: 客戶網路存取（公網 HTTPS + 網域 + Cloudflare）
├── Week 2-3: Backup 強化（hourly + 異地 + 加密）
└── Week 3-4: Audit log 結構化 + Multi-tenant 修補

Month 2
├── Week 1: Admin recovery flow
├── Week 2-3: TOTP MFA
└── Week 4: Compliance 文件（律師起草開始）

Month 3
├── Week 1-2: HA 雙機 streaming replication
├── Week 2-3: Monitoring + Alerting
└── Week 4: Customer onboarding (signup + admin console)

Month 4
├── Week 1-2: SAML SSO
├── Week 3-4: Stripe + billing
└── 第一個外部客戶簽約

Month 5-6
└── 修第一波客戶 feedback
```

---

## 12. Phase 2 入場前的 Go/No-Go 清單

簽第一個外部付費客戶前，至少要完成：

**必過**：
- [ ] HA 至少有「異地備援」（方案 B）
- [ ] Public HTTPS 走自有網域 + TLS
- [ ] DPA / Privacy Policy 律師審過
- [ ] Audit log 全面 emit + 客戶可查
- [ ] Admin recovery flow
- [ ] Backup 加密 + 跨地副本
- [ ] Multi-tenant audit 全綠（含 meetings.rs 修完）
- [ ] TOTP MFA 至少有
- [ ] 7×24 監測 + Slack alert
- [ ] Status page

**強烈建議**：
- [ ] SAML SSO（如果客戶要）
- [ ] 雙機 HA
- [ ] SOC 2 Type I 開始

**可延後**：
- [ ] N-of-M admin recovery
- [ ] WebAuthn
- [ ] 客戶 self-serve billing
- [ ] i18n

---

## 附錄：Phase 1 留下的 audit 結論摘要

完整版見 [phase-1-implementation.md §10](./phase-1-implementation.md#10-今日-audit-發現與修正)。

| 項目 | Phase 1 結果 | Phase 2 待辦 |
|---|---|---|
| 10 條加密測試 | ✅ 全綠 | 自動化測試（CI 跑） |
| Bug fix | ✅ `/bin/mount` + startup sweep | — |
| Multi-tenant audit | ✅ 11/11 handler 通過 | meetings cross-tenant + 5 個未深查 |
| Windows E2E | ✅ 透過 Tailscale 通 | 公網存取 |
| Backup | ✅ 03:00 自動跑 | 異地 + 加密 + HA |
| Firewall | ✅ Tailscale-only bind | 公網 + pfctl 更嚴格 |
