"""
LINE Messaging API 測試腳本
測試 LINE 推播訊息功能
"""

import os
from dotenv import load_dotenv

# 載入環境變數
load_dotenv()

def test_line_messaging():
    """測試 LINE Messaging API 連線"""
    
    print("=" * 50)
    print("LINE Messaging API 測試")
    print("=" * 50)
    
    # 1. 檢查環境變數
    channel_token = os.getenv("LINE_CHANNEL_ACCESS_TOKEN")
    user_id = os.getenv("LINE_USER_ID")
    
    print("\n1. 檢查環境變數...")
    if channel_token:
        print(f"   ✓ LINE_CHANNEL_ACCESS_TOKEN: {channel_token[:20]}...")
    else:
        print("   ✗ LINE_CHANNEL_ACCESS_TOKEN 未設定")
        
    if user_id:
        print(f"   ✓ LINE_USER_ID: {user_id}")
    else:
        print("   ✗ LINE_USER_ID 未設定")
    
    if not channel_token or not user_id:
        print("\n❌ 請先設定 .env 檔案中的 LINE 環境變數")
        print("\n設定步驟：")
        print("1. 前往 https://developers.line.biz/console/")
        print("2. 建立 Provider 和 Messaging API Channel")
        print("3. 在 Channel 設定中取得 Channel Access Token")
        print("4. 複製 Your user ID（在 Basic settings）")
        print("5. 將這些值填入 .env 檔案")
        return False
    
    # 2. 測試發送訊息
    print("\n2. 嘗試發送測試訊息...")
    
    from connector import LINEMessagingConnector
    
    connector = LINEMessagingConnector()
    result = connector.send("🧪 Moltbot 測試訊息 - LINE Messaging API 連線成功！")
    
    if result.success:
        print("   ✓ 訊息發送成功！")
        print("\n✅ LINE Messaging API 測試完成！")
        return True
    else:
        print(f"   ✗ 發送失敗: {result.error_message}")
        return False

if __name__ == "__main__":
    test_line_messaging()
