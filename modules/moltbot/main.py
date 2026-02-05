"""
Moltbot News Crawler - Main Entry Point
新聞爬取判讀系統主程式
"""

import os
from dotenv import load_dotenv

# 載入環境變數
load_dotenv()

# 匯入模塊
from crawler import NewsCrawler, CrawlResult
from filter import TextFilter, FilteredContent
from analyzer import KeywordAnalyzer, AnalysisResult
from connector import (
    create_llm_connector, 
    create_messaging_connector,
    LLMProvider, 
    MessagingPlatform
)


class MoltbotNewsCrawler:
    """Moltbot 新聞爬取判讀系統"""
    
    def __init__(
        self, 
        llm_provider: LLMProvider = LLMProvider.GEMINI,
        messaging_platform: MessagingPlatform = None
    ):
        """
        初始化系統
        
        Args:
            llm_provider: LLM 服務提供者
            messaging_platform: 通訊平台（可選）
        """
        self.crawler = NewsCrawler()
        self.filter = TextFilter()
        self.analyzer = KeywordAnalyzer()
        self.llm = create_llm_connector(llm_provider)
        
        self.messenger = None
        if messaging_platform:
            self.messenger = create_messaging_connector(messaging_platform)
    
    def process(self, url: str, send_notification: bool = False) -> dict:
        """
        處理單一新聞 URL
        
        Args:
            url: 新聞網址
            send_notification: 是否發送通知
            
        Returns:
            dict: 處理結果
        """
        print(f"\n{'='*60}")
        print(f"🔍 處理 URL: {url}")
        print('='*60)
        
        # Step 1: 爬取網頁
        print("\n📥 Step 1: 爬取網頁...")
        crawl_result = self.crawler.fetch(url)
        if not crawl_result.success:
            return {"error": f"爬取失敗: {crawl_result.error_message}"}
        print(f"   ✅ 成功抓取 {len(crawl_result.html)} 字元")
        
        # Step 2: 過濾內容
        print("\n🧹 Step 2: 過濾 HTML...")
        filtered = self.filter.extract(crawl_result.html)
        print(f"   ✅ 標題: {filtered.title[:50]}...")
        print(f"   ✅ 段落數: {len(filtered.paragraphs)}")
        
        # Step 3: 關鍵字分析
        print("\n📊 Step 3: 關鍵字分析...")
        analysis = self.analyzer.analyze(filtered.body)
        print(f"   ✅ 總詞數: {analysis.total_words}")
        print(f"   ✅ 獨特詞: {analysis.unique_words}")
        top_5 = [f"{w}({c})" for w, c in analysis.keywords[:5]]
        print(f"   ✅ Top 5: {', '.join(top_5)}")
        
        # Step 4: LLM 摘要
        print("\n🤖 Step 4: AI 摘要生成...")
        summary = self.llm.summarize(filtered.body[:3000])  # 限制長度
        print(f"   ✅ 摘要: {summary[:100]}...")
        
        # Step 5: 發送通知（可選）
        if send_notification and self.messenger:
            print("\n📤 Step 5: 發送通知...")
            message = f"📰 {filtered.title}\n\n{summary}"
            result = self.messenger.send(message)
            print(f"   {'✅' if result.success else '❌'} {result.platform}: {result.error_message or '成功'}")
        
        print(f"\n{'='*60}")
        print("✅ 處理完成!")
        print('='*60)
        
        return {
            "url": url,
            "title": filtered.title,
            "summary": summary,
            "keywords": analysis.keywords[:10],
            "entities": analysis.top_entities,
            "word_count": analysis.total_words
        }


def main():
    """主程式入口"""
    print("""
    ╔══════════════════════════════════════════════════════════╗
    ║           🦞 Moltbot News Crawler v1.0                   ║
    ║           新聞爬取判讀系統                                 ║
    ╚══════════════════════════════════════════════════════════╝
    """)
    
    # 建立系統實例
    bot = MoltbotNewsCrawler(
        llm_provider=LLMProvider.GEMINI,
        messaging_platform=None  # 先不啟用通知
    )
    
    # 測試 URL
    test_urls = [
        "https://news.yahoo.co.jp/",
    ]
    
    for url in test_urls:
        result = bot.process(url)
        if "error" not in result:
            print(f"\n📋 結果摘要:")
            print(f"   標題: {result['title']}")
            print(f"   關鍵字: {[k[0] for k in result['keywords'][:5]]}")


if __name__ == "__main__":
    main()
