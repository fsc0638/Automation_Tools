#!/bin/bash
# 測試 LINE-OpenClaw Bridge 的訊息處理

echo "測試 LINE Webhook 模擬訊息..."

# LINE Webhook 請求格式
# 注意：這是一個簡化的測試，實際需要正確的 HMAC 簽名
curl -X POST http://localhost:3000/callback \
  -H "Content-Type: application/json" \
  -H "X-Line-Signature: test_signature_for_development" \
  -d '{
    "events": [{
      "type": "message",
      "message": {
        "type": "text",
        "id": "test123",
        "text": "你好，OpenClaw！"
      },
      "source": {
        "type": "user",
        "userId": "test_user_001"
      },
      "replyToken": "test_reply_token",
      "timestamp": 1612345678000
    }]
  }'

echo -e "\n\n測試完成！"
