"""
LINE Bot Webhook 伺服器
處理用戶訊息並提供互動式新聞摘要功能
"""

import os
import re
import threading
from flask import Flask, request, abort
from dotenv import load_dotenv

from linebot.v3 import WebhookHandler
from linebot.v3.exceptions import InvalidSignatureError
from linebot.v3.messaging import (
    Configuration,
    ApiClient,
    MessagingApi,
    ReplyMessageRequest,
    PushMessageRequest,
    TextMessage,
    QuickReply,
    QuickReplyItem,
    PostbackAction,
    FlexMessage,
    FlexBubble,
    FlexBox,
    FlexText,
    FlexButton,
    URIAction
)
from linebot.v3.webhooks import (
    MessageEvent,
    PostbackEvent,
    TextMessageContent
)

# 載入環境變數
load_dotenv()

# Flask 應用
app = Flask(__name__)

# LINE Bot 設定
CHANNEL_ACCESS_TOKEN = os.getenv("LINE_CHANNEL_ACCESS_TOKEN")
CHANNEL_SECRET = os.getenv("LINE_CHANNEL_SECRET")

if not CHANNEL_ACCESS_TOKEN or not CHANNEL_SECRET:
    raise ValueError("請設定 LINE_CHANNEL_ACCESS_TOKEN 和 LINE_CHANNEL_SECRET 環境變數")

configuration = Configuration(access_token=CHANNEL_ACCESS_TOKEN)
handler = WebhookHandler(CHANNEL_SECRET)

# 關鍵字模式（支援多語言）
NEWS_KEYWORDS = [
    r"新聞摘要",
    r"新聞",
    r"ニュース",
    r"news",
    r"摘要",
    r"頭條",
    r"今日新聞"
]

# 新聞來源設定
NEWS_SOURCES = {
    "yahoo": {
        "name": "Yahoo 新聞",
        "url": "https://tw.news.yahoo.com/",
        "emoji": "📰"
    },
    "nikkei": {
        "name": "日經新聞",
        "url": "https://www.nikkei.com/",
        "emoji": "📊"
    }
}


def is_news_request(text: str) -> bool:
    """檢查訊息是否為新聞摘要請求"""
    text_lower = text.lower().strip()
    for keyword in NEWS_KEYWORDS:
        if re.search(keyword, text_lower, re.IGNORECASE):
            return True
    return False


def create_news_source_quick_reply() -> QuickReply:
    """建立新聞來源選擇的 Quick Reply"""
    items = []
    for source_id, source_info in NEWS_SOURCES.items():
        items.append(
            QuickReplyItem(
                action=PostbackAction(
                    label=f"{source_info['emoji']} {source_info['name']}",
                    data=f"news_source={source_id}",
                    display_text=f"我想看 {source_info['name']}"
                )
            )
        )
    return QuickReply(items=items)


def create_news_flex_message(title: str, summary: str, url: str, source_name: str) -> FlexMessage:
    """建立新聞摘要的 Flex Message 卡片"""
    bubble = FlexBubble(
        header=FlexBox(
            layout="vertical",
            contents=[
                FlexText(text=f"📰 {source_name}", weight="bold", size="lg", color="#1DB446")
            ]
        ),
        body=FlexBox(
            layout="vertical",
            contents=[
                FlexText(text=title, weight="bold", size="md", wrap=True),
                FlexText(text=summary, size="sm", color="#666666", wrap=True, margin="md")
            ]
        ),
        footer=FlexBox(
            layout="vertical",
            contents=[
                FlexButton(
                    action=URIAction(label="閱讀全文", uri=url),
                    style="primary",
                    color="#1DB446"
                )
            ]
        )
    )
    return FlexMessage(alt_text=f"{source_name}新聞摘要", contents=bubble)


def process_news_request(user_id: str, source_id: str):
    """處理新聞請求（在背景執行）"""
    from crawler import NewsCrawler
    from filter import TextFilter
    from connector import create_llm_connector, LLMProvider
    
    source_info = NEWS_SOURCES.get(source_id)
    if not source_info:
        return
    
    with ApiClient(configuration) as api_client:
        line_bot_api = MessagingApi(api_client)
        
        # 發送處理中訊息
        line_bot_api.push_message(
            PushMessageRequest(
                to=user_id,
                messages=[TextMessage(text=f"⏳ 正在爬取 {source_info['name']}，請稍候...")]
            )
        )
        
        try:
            # 執行爬蟲
            crawler = NewsCrawler()
            crawl_result = crawler.fetch(source_info["url"])
            
            if not crawl_result.success:
                line_bot_api.push_message(
                    PushMessageRequest(
                        to=user_id,
                        messages=[TextMessage(text=f"❌ 爬取失敗：{crawl_result.error_message}")]
                    )
                )
                return
            
            # 過濾文字
            text_filter = TextFilter()
            clean_text = text_filter.extract_main_content(crawl_result.html)
            
            # 使用 LLM 摘要
            llm = create_llm_connector(LLMProvider.GEMINI)
            summary_result = llm.generate(
                prompt=f"請用繁體中文，將以下新聞內容摘要成 3-5 個重點，每個重點一行：\n\n{clean_text[:3000]}"
            )
            
            if summary_result.success:
                # 發送摘要結果
                line_bot_api.push_message(
                    PushMessageRequest(
                        to=user_id,
                        messages=[
                            TextMessage(
                                text=f"📰 {source_info['name']} 摘要\n\n{summary_result.content}\n\n🔗 來源: {source_info['url']}"
                            )
                        ]
                    )
                )
            else:
                line_bot_api.push_message(
                    PushMessageRequest(
                        to=user_id,
                        messages=[TextMessage(text=f"❌ 摘要生成失敗：{summary_result.error_message}")]
                    )
                )
                
        except Exception as e:
            line_bot_api.push_message(
                PushMessageRequest(
                    to=user_id,
                    messages=[TextMessage(text=f"❌ 處理時發生錯誤：{str(e)}")]
                )
            )


@app.route("/callback", methods=["POST"])
def callback():
    """LINE Webhook 回調端點"""
    signature = request.headers.get("X-Line-Signature", "")
    body = request.get_data(as_text=True)
    
    app.logger.info(f"Request body: {body}")
    
    try:
        handler.handle(body, signature)
    except InvalidSignatureError:
        app.logger.error("Invalid signature")
        abort(400)
    
    return "OK"


@handler.add(MessageEvent, message=TextMessageContent)
def handle_text_message(event: MessageEvent):
    """處理文字訊息"""
    user_message = event.message.text
    user_id = event.source.user_id
    
    with ApiClient(configuration) as api_client:
        line_bot_api = MessagingApi(api_client)
        
        if is_news_request(user_message):
            # 發送新聞來源選擇
            line_bot_api.reply_message(
                ReplyMessageRequest(
                    reply_token=event.reply_token,
                    messages=[
                        TextMessage(
                            text="請選擇您想看的新聞來源：",
                            quick_reply=create_news_source_quick_reply()
                        )
                    ]
                )
            )
        else:
            # 一般回覆
            line_bot_api.reply_message(
                ReplyMessageRequest(
                    reply_token=event.reply_token,
                    messages=[
                        TextMessage(
                            text=f"您好！輸入「新聞摘要」可以取得最新新聞。\n\n您說的是：{user_message}"
                        )
                    ]
                )
            )


@handler.add(PostbackEvent)
def handle_postback(event: PostbackEvent):
    """處理 Postback 事件（按鈕點擊）"""
    data = event.postback.data
    user_id = event.source.user_id
    
    if data.startswith("news_source="):
        source_id = data.split("=")[1]
        
        with ApiClient(configuration) as api_client:
            line_bot_api = MessagingApi(api_client)
            
            # 先回覆確認訊息
            source_name = NEWS_SOURCES.get(source_id, {}).get("name", "新聞")
            line_bot_api.reply_message(
                ReplyMessageRequest(
                    reply_token=event.reply_token,
                    messages=[TextMessage(text=f"✅ 已選擇 {source_name}")]
                )
            )
        
        # 在背景執行新聞處理
        thread = threading.Thread(target=process_news_request, args=(user_id, source_id))
        thread.start()


@app.route("/health", methods=["GET"])
def health_check():
    """健康檢查端點"""
    return {"status": "ok", "service": "moltbot-line-bot"}


if __name__ == "__main__":
    port = int(os.getenv("PORT", 5000))
    print(f"🚀 LINE Bot 伺服器啟動中... http://localhost:{port}")
    print(f"📌 Webhook URL: http://your-domain:{port}/callback")
    print("\n💡 提示：使用 ngrok 建立公開 URL：ngrok http {port}")
    app.run(host="0.0.0.0", port=port, debug=True)
