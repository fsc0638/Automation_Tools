#!/bin/bash
# LINE-OpenClaw Bridge 啟動腳本 (帶詳細日誌)

echo "🚀 啟動 LINE-OpenClaw Bridge..."
echo "📊 日誌等級: DEBUG"
echo "================================"
echo ""

# 設定詳細日誌等級
export RUST_LOG=info,line_openclaw_bridge=debug

# 啟動 Bridge (前台模式，可以看到即時日誌)
./target/release/line-openclaw-bridge
