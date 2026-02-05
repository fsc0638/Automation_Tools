"""
Moltbot News Crawler Module
數據爬蟲模塊 - 負責抓取新聞網頁內容
"""

import requests
from typing import Optional, Dict, Any
from dataclasses import dataclass


@dataclass
class CrawlResult:
    """爬取結果的資料結構"""
    url: str
    html: str
    status_code: int
    success: bool
    error_message: Optional[str] = None


class NewsCrawler:
    """新聞爬蟲類別 - 支援多種來源"""
    
    DEFAULT_HEADERS = {
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8",
        "Accept-Language": "zh-TW,zh;q=0.9,en-US;q=0.8,en;q=0.7",
    }

    def __init__(self, timeout: int = 30, headers: Optional[Dict[str, str]] = None):
        """
        初始化爬蟲
        
        Args:
            timeout: 請求超時秒數
            headers: 自訂 HTTP 標頭
        """
        self.timeout = timeout
        self.headers = headers or self.DEFAULT_HEADERS
        self.session = requests.Session()
        self.session.headers.update(self.headers)

    def fetch(self, url: str) -> CrawlResult:
        """
        抓取指定 URL 的網頁內容
        
        Args:
            url: 目標網址
            
        Returns:
            CrawlResult: 包含 HTML 內容與狀態的結果物件
        """
        try:
            response = self.session.get(url, timeout=self.timeout)
            return CrawlResult(
                url=url,
                html=response.text,
                status_code=response.status_code,
                success=response.status_code == 200
            )
        except requests.RequestException as e:
            return CrawlResult(
                url=url,
                html="",
                status_code=0,
                success=False,
                error_message=str(e)
            )

    def fetch_multiple(self, urls: list[str]) -> list[CrawlResult]:
        """
        批次抓取多個 URL
        
        Args:
            urls: 網址列表
            
        Returns:
            list[CrawlResult]: 結果列表
        """
        return [self.fetch(url) for url in urls]


# 範例使用
if __name__ == "__main__":
    crawler = NewsCrawler()
    result = crawler.fetch("https://news.yahoo.co.jp/")
    
    print(f"URL: {result.url}")
    print(f"Success: {result.success}")
    print(f"Status Code: {result.status_code}")
    print(f"HTML Length: {len(result.html)} characters")
