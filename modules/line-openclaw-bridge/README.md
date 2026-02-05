# LINE-OpenClaw Bridge

連接 LINE Bot 和本地 OpenClaw AI 助理的 Rust 服務。

## 功能

- ✅ LINE Webhook 接收與驗證
- ✅ HMAC-SHA256 簽名驗證
- ✅ 訊息轉發至 OpenClaw
- ✅ OpenClaw 回應發送回 LINE
- ✅ 健康檢查端點

## 安裝

### 前置需求

- Rust 1.75+
- OpenClaw 已在本地運行 (port 18789)
- LINE Messaging API Channel

### 編譯

```bash
cargo build --release
```

### 設定

複製 `.env.example` 為 `.env` 並填入設定：

```bash
cp .env.example .env
```

編輯 `.env`：

```env
LINE_CHANNEL_ACCESS_TOKEN=your_token
LINE_CHANNEL_SECRET=your_secret
OPENCLAW_BASE_URL=http://127.0.0.1:18789
```

## 執行

```bash
cargo run
```

服務會在 `http://localhost:3000` 啟動。

## Webhook 設定

1. 使用 ngrok 建立公開 URL：
   ```bash
   ngrok http 3000
   ```

2. 到 [LINE Developers Console](https://developers.line.biz/) 設定 Webhook URL：
   ```
   https://xxx.ngrok-free.app/callback
   ```

3. 啟用「Use webhook」並關閉「Auto-reply messages」

## API 端點

| 端點 | 方法 | 說明 |
|------|------|------|
| `/` | GET | 服務資訊 |
| `/health` | GET | 健康檢查 |
| `/callback` | POST | LINE Webhook |

## 專案結構

```
line-openclaw-bridge/
├── Cargo.toml          # 專案設定
├── .env.example        # 環境變數範例
├── README.md           # 說明文件
└── src/
    ├── main.rs         # 主程式
    ├── line.rs         # LINE API 客戶端
    └── openclaw.rs     # OpenClaw 客戶端
```
