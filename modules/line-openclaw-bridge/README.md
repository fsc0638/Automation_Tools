# LINE-OpenClaw Bridge

連接 LINE Bot 和本地 OpenClaw AI 助理的 Rust 服務。

## 🚀 已更新功能

- ✅ **OpenAI Chat Completions 支援**：已將對接端點更新為 `/v1/chat/completions`，相容 OpenAI 標準格式。
- ✅ **Thinking Model 優化**：增加逾時時間至 60 秒，支援處理週期較長的思考型模型。
- ✅ **精準日誌**：優化了 OpenClaw 回應與錯誤的捕捉日誌。
- ✅ **偵錯工具**：新增 `start_with_logs.sh` 與 `test_webhook.sh`。

## 🛠️ 前置需求

- Rust 1.75+
- OpenClaw 已在本地運行 (port 18789)
- **OpenClaw 設定**：須在 `openclaw.json` 中啟用 `gateway.http.endpoints.chatCompletions`。

## 🔧 設定與執行

### 1. 複製並設定環境變數
```bash
cp .env.example .env
```
編輯 `.env` 中的 `LINE_CHANNEL_ACCESS_TOKEN`、`LINE_CHANNEL_SECRET` 與 `OPENCLAW_GATEWAY_TOKEN`。

### 2. 執行服務 (開發排錯模式)
建議使用以下指令啟動，以即時看到對話 Log：
```bash
./start_with_logs.sh
```

## ⚠️ 重要注意事項與排錯 (Troubleshooting)

### 1. 出現 401 Unauthorized
*   **原因**：通常是 `Channel Secret` 或 `Gateway Token` 不匹配。
*   **解決**：確保 `.env` 中的 `LINE_CHANNEL_SECRET` 與 LINE Developers Console 完全一致。若直接使用 OpenClaw 原生 Webhook，請確保 `openclaw.json` 中的 `channels.line.channelSecret` 也已更新並**重啟 OpenClaw**。

### 2. 出現 Connection Refused (os error 61)
*   **原因**：OpenClaw Gateway 未啟動或失敗。
*   **常見失敗原因**：若 `openclaw.json` 誤啟用了 `tailscale.mode: "funnel"` 但未設定密碼，Gateway 會啟動失敗。請確認該項設為 `"off"`。
*   **手動啟動指令**：`openclaw gateway --port 18789 --bind loopback --force &`

### 3. Webhook URL 設定
*   ** ngrok 轉發**：`ngrok http 3000` (使用 Bridge) 或 `ngrok http 18789` (使用 OpenClaw 原生)。
*   **路徑**：務必在 URL 後方加上 `/callback`。

## 📂 專案結構

```
line-openclaw-bridge/
├── .env.example        # 環境變數範例
├── start_with_logs.sh  # 帶有日誌的啟動指令碼
├── test_webhook.sh     # Webhook 本地模擬測試工具
└── src/
    ├── main.rs         # 核心 Web 伺服器
    ├── line.rs         # LINE API 整合
    └── openclaw.rs     # OpenClaw 溝通邏輯 (支援 OpenAI 格式)
```
