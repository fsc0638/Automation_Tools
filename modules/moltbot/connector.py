"""
Moltbot Generic API Connector Module
通用型 API 串接工具 - 支援 LLM 與通訊平台的彈性切換
"""

import os
import json
import requests
from abc import ABC, abstractmethod
from typing import Optional, Dict, Any, List
from dataclasses import dataclass
from enum import Enum
from dotenv import load_dotenv

# 載入 .env 環境變數
load_dotenv()


# ============================================================
# 基礎介面定義
# ============================================================

class LLMProvider(Enum):
    """LLM 服務提供者"""
    OPENAI = "openai"
    GEMINI = "gemini"


class MessagingPlatform(Enum):
    """通訊平台"""
    DISCORD = "discord"
    TELEGRAM = "telegram"
    SLACK = "slack"
    LINE = "line"


@dataclass
class LLMResponse:
    """LLM 回應結構"""
    content: str
    model: str
    tokens_used: int
    success: bool
    error_message: Optional[str] = None


@dataclass
class MessageResult:
    """訊息發送結果"""
    platform: str
    success: bool
    message_id: Optional[str] = None
    error_message: Optional[str] = None


# ============================================================
# LLM 連接器抽象類別
# ============================================================

class BaseLLMConnector(ABC):
    """LLM 連接器基礎類別"""
    
    @abstractmethod
    def generate(self, prompt: str, system_prompt: Optional[str] = None) -> LLMResponse:
        """生成回應"""
        pass
    
    @abstractmethod
    def summarize(self, text: str) -> str:
        """摘要文字"""
        pass


class OpenAIConnector(BaseLLMConnector):
    """OpenAI API 連接器"""
    
    def __init__(self, api_key: Optional[str] = None, model: str = "gpt-4o-mini"):
        self.api_key = api_key or os.getenv("OPENAI_API_KEY")
        self.model = model
        self.base_url = "https://api.openai.com/v1/chat/completions"
    
    def generate(self, prompt: str, system_prompt: Optional[str] = None) -> LLMResponse:
        if not self.api_key:
            return LLMResponse("", self.model, 0, False, "API key not configured")
        
        messages = []
        if system_prompt:
            messages.append({"role": "system", "content": system_prompt})
        messages.append({"role": "user", "content": prompt})
        
        try:
            response = requests.post(
                self.base_url,
                headers={
                    "Authorization": f"Bearer {self.api_key}",
                    "Content-Type": "application/json"
                },
                json={
                    "model": self.model,
                    "messages": messages,
                    "temperature": 0.7
                },
                timeout=60
            )
            data = response.json()
            
            if "error" in data:
                return LLMResponse("", self.model, 0, False, data["error"]["message"])
            
            return LLMResponse(
                content=data["choices"][0]["message"]["content"],
                model=self.model,
                tokens_used=data.get("usage", {}).get("total_tokens", 0),
                success=True
            )
        except Exception as e:
            return LLMResponse("", self.model, 0, False, str(e))
    
    def summarize(self, text: str) -> str:
        prompt = f"請用繁體中文簡要摘要以下新聞內容（約 100 字）：\n\n{text}"
        result = self.generate(prompt)
        return result.content if result.success else f"摘要失敗: {result.error_message}"


class GeminiConnector(BaseLLMConnector):
    """Google Gemini API 連接器"""
    
    def __init__(self, api_key: Optional[str] = None, model: str = "gemini-2.0-flash"):
        self.api_key = api_key or os.getenv("GOOGLE_API_KEY")
        self.model = model
        self.base_url = f"https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
    
    def generate(self, prompt: str, system_prompt: Optional[str] = None) -> LLMResponse:
        if not self.api_key:
            return LLMResponse("", self.model, 0, False, "API key not configured")
        
        full_prompt = f"{system_prompt}\n\n{prompt}" if system_prompt else prompt
        
        try:
            response = requests.post(
                f"{self.base_url}?key={self.api_key}",
                headers={"Content-Type": "application/json"},
                json={
                    "contents": [{"parts": [{"text": full_prompt}]}]
                },
                timeout=60
            )
            data = response.json()
            
            if "error" in data:
                return LLMResponse("", self.model, 0, False, data["error"]["message"])
            
            content = data["candidates"][0]["content"]["parts"][0]["text"]
            return LLMResponse(
                content=content,
                model=self.model,
                tokens_used=0,  # Gemini API 不直接返回 token 數
                success=True
            )
        except Exception as e:
            return LLMResponse("", self.model, 0, False, str(e))
    
    def summarize(self, text: str) -> str:
        prompt = f"請用繁體中文簡要摘要以下新聞內容（約 100 字）：\n\n{text}"
        result = self.generate(prompt)
        return result.content if result.success else f"摘要失敗: {result.error_message}"


# ============================================================
# 通訊平台連接器
# ============================================================

class BaseMessagingConnector(ABC):
    """通訊平台連接器基礎類別"""
    
    @abstractmethod
    def send(self, message: str, **kwargs) -> MessageResult:
        """發送訊息"""
        pass


class DiscordWebhookConnector(BaseMessagingConnector):
    """Discord Webhook 連接器"""
    
    def __init__(self, webhook_url: Optional[str] = None):
        self.webhook_url = webhook_url or os.getenv("DISCORD_WEBHOOK_URL")
    
    def send(self, message: str, **kwargs) -> MessageResult:
        if not self.webhook_url:
            return MessageResult("discord", False, error_message="Webhook URL not configured")
        
        try:
            response = requests.post(
                self.webhook_url,
                json={"content": message},
                timeout=10
            )
            return MessageResult(
                platform="discord",
                success=response.status_code in [200, 204],
                error_message=None if response.ok else response.text
            )
        except Exception as e:
            return MessageResult("discord", False, error_message=str(e))


class TelegramConnector(BaseMessagingConnector):
    """Telegram Bot 連接器"""
    
    def __init__(self, bot_token: Optional[str] = None, chat_id: Optional[str] = None):
        self.bot_token = bot_token or os.getenv("TELEGRAM_BOT_TOKEN")
        self.chat_id = chat_id or os.getenv("TELEGRAM_CHAT_ID")
    
    def send(self, message: str, **kwargs) -> MessageResult:
        if not self.bot_token or not self.chat_id:
            return MessageResult("telegram", False, error_message="Bot token or chat ID not configured")
        
        try:
            url = f"https://api.telegram.org/bot{self.bot_token}/sendMessage"
            response = requests.post(
                url,
                json={"chat_id": self.chat_id, "text": message, "parse_mode": "HTML"},
                timeout=10
            )
            data = response.json()
            return MessageResult(
                platform="telegram",
                success=data.get("ok", False),
                message_id=str(data.get("result", {}).get("message_id")),
                error_message=data.get("description")
            )
        except Exception as e:
            return MessageResult("telegram", False, error_message=str(e))


class SlackWebhookConnector(BaseMessagingConnector):
    """Slack Webhook 連接器"""
    
    def __init__(self, webhook_url: Optional[str] = None):
        self.webhook_url = webhook_url or os.getenv("SLACK_WEBHOOK_URL")
    
    def send(self, message: str, **kwargs) -> MessageResult:
        if not self.webhook_url:
            return MessageResult("slack", False, error_message="Webhook URL not configured")
        
        try:
            response = requests.post(
                self.webhook_url,
                json={"text": message},
                timeout=10
            )
            return MessageResult(
                platform="slack",
                success=response.status_code == 200,
                error_message=None if response.ok else response.text
            )
        except Exception as e:
            return MessageResult("slack", False, error_message=str(e))


class LINEMessagingConnector(BaseMessagingConnector):
    """LINE Messaging API 連接器（台灣常用通訊軟體）
    
    注意：LINE Notify 已於 2025 年 3 月 31 日終止服務，
    本連接器使用 LINE Messaging API 作為替代方案。
    
    設定步驟：
    1. 前往 https://developers.line.biz/console/
    2. 建立 Provider 和 Messaging API Channel
    3. 在 Channel 設定中取得 Channel Access Token
    4. 將 Token 設定到環境變數 LINE_CHANNEL_ACCESS_TOKEN
    5. 將要推播的用戶 ID 設定到 LINE_USER_ID
       （可從 Webhook 事件取得，或使用 LINE Official Account 的用戶 ID）
    """
    
    PUSH_API_URL = "https://api.line.me/v2/bot/message/push"
    
    def __init__(self, channel_access_token: Optional[str] = None, user_id: Optional[str] = None):
        self.channel_access_token = channel_access_token or os.getenv("LINE_CHANNEL_ACCESS_TOKEN")
        self.user_id = user_id or os.getenv("LINE_USER_ID")
    
    def _send_messages(self, messages: list, user_id: Optional[str] = None) -> MessageResult:
        """發送多個訊息"""
        target_user = user_id or self.user_id
        
        if not self.channel_access_token:
            return MessageResult("line", False, error_message="LINE Channel Access Token not configured")
        if not target_user:
            return MessageResult("line", False, error_message="LINE User ID not configured")
        
        try:
            headers = {
                "Authorization": f"Bearer {self.channel_access_token}",
                "Content-Type": "application/json"
            }
            
            payload = {
                "to": target_user,
                "messages": messages
            }
            
            response = requests.post(
                self.PUSH_API_URL,
                headers=headers,
                json=payload,
                timeout=10
            )
            
            if response.status_code == 200:
                return MessageResult(platform="line", success=True)
            else:
                error_data = response.json()
                return MessageResult(
                    platform="line",
                    success=False,
                    error_message=error_data.get("message", response.text)
                )
        except Exception as e:
            return MessageResult("line", False, error_message=str(e))
    
    def send(self, message: str, **kwargs) -> MessageResult:
        """
        發送 LINE 推播訊息
        
        Args:
            message: 訊息內容
            **kwargs: 額外參數
                - user_id: 指定接收者（覆蓋預設值）
        """
        messages = [{"type": "text", "text": message}]
        return self._send_messages(messages, kwargs.get("user_id"))
    
    def send_to_group(self, message: str, group_id: str) -> MessageResult:
        """發送訊息到群組"""
        return self.send(message, user_id=group_id)
    
    def send_image(self, image_url: str, preview_url: Optional[str] = None, **kwargs) -> MessageResult:
        """
        發送圖片訊息
        
        Args:
            image_url: 圖片網址（HTTPS）
            preview_url: 預覽圖網址（選用，預設使用 image_url）
        """
        messages = [{
            "type": "image",
            "originalContentUrl": image_url,
            "previewImageUrl": preview_url or image_url
        }]
        return self._send_messages(messages, kwargs.get("user_id"))
    
    def send_with_quick_reply(self, message: str, options: list[dict], **kwargs) -> MessageResult:
        """
        發送帶有快速回覆按鈕的訊息
        
        Args:
            message: 訊息內容
            options: 選項列表，每個選項為 dict，包含 label 和 text/data
                例如: [{"label": "選項1", "text": "回覆1"}, {"label": "選項2", "data": "action=2"}]
        """
        items = []
        for opt in options[:13]:  # LINE 限制最多 13 個
            action = {}
            if "data" in opt:
                action = {
                    "type": "postback",
                    "label": opt["label"],
                    "data": opt["data"],
                    "displayText": opt.get("display_text", opt["label"])
                }
            else:
                action = {
                    "type": "message",
                    "label": opt["label"],
                    "text": opt.get("text", opt["label"])
                }
            items.append({"type": "action", "action": action})
        
        messages = [{
            "type": "text",
            "text": message,
            "quickReply": {"items": items}
        }]
        return self._send_messages(messages, kwargs.get("user_id"))
    
    def send_flex_message(self, alt_text: str, contents: dict, **kwargs) -> MessageResult:
        """
        發送 Flex Message（自訂卡片）
        
        Args:
            alt_text: 替代文字（在通知和不支援的裝置上顯示）
            contents: Flex Message 內容（bubble 或 carousel 格式）
        """
        messages = [{
            "type": "flex",
            "altText": alt_text,
            "contents": contents
        }]
        return self._send_messages(messages, kwargs.get("user_id"))


# ============================================================
# 工廠函式
# ============================================================

def create_llm_connector(provider: LLMProvider, **kwargs) -> BaseLLMConnector:
    """
    建立 LLM 連接器（工廠模式）
    
    Args:
        provider: LLM 服務提供者
        **kwargs: 額外參數（如 api_key, model）
        
    Returns:
        BaseLLMConnector: LLM 連接器實例
    """
    if provider == LLMProvider.OPENAI:
        return OpenAIConnector(**kwargs)
    elif provider == LLMProvider.GEMINI:
        return GeminiConnector(**kwargs)
    else:
        raise ValueError(f"Unsupported LLM provider: {provider}")


def create_messaging_connector(platform: MessagingPlatform, **kwargs) -> BaseMessagingConnector:
    """
    建立通訊平台連接器（工廠模式）
    
    Args:
        platform: 通訊平台
        **kwargs: 額外參數（如 webhook_url, bot_token）
        
    Returns:
        BaseMessagingConnector: 通訊連接器實例
    """
    if platform == MessagingPlatform.DISCORD:
        return DiscordWebhookConnector(**kwargs)
    elif platform == MessagingPlatform.TELEGRAM:
        return TelegramConnector(**kwargs)
    elif platform == MessagingPlatform.SLACK:
        return SlackWebhookConnector(**kwargs)
    elif platform == MessagingPlatform.LINE:
        return LINEMessagingConnector(**kwargs)
    else:
        raise ValueError(f"Unsupported messaging platform: {platform}")


# ============================================================
# 範例使用
# ============================================================

if __name__ == "__main__":
    # 範例：建立 LLM 連接器
    print("=== LLM Connector Demo ===")
    llm = create_llm_connector(LLMProvider.GEMINI)
    print(f"Created LLM connector: {type(llm).__name__}")
    
    # 範例：建立通訊連接器
    print("\n=== Messaging Connector Demo ===")
    messenger = create_messaging_connector(MessagingPlatform.DISCORD)
    print(f"Created messaging connector: {type(messenger).__name__}")
    
    print("\n✅ Connector module loaded successfully!")
