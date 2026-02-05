"""
Moltbot Text Filter Module
文字過濾器模塊 - 清理 HTML 並提取純文字正文
"""

from bs4 import BeautifulSoup
from typing import Optional, List
from dataclasses import dataclass
import re


@dataclass
class FilteredContent:
    """過濾後的內容結構"""
    title: str
    body: str
    paragraphs: List[str]
    word_count: int


class TextFilter:
    """文字過濾器 - 從 HTML 提取乾淨的正文"""
    
    # 需要移除的標籤
    REMOVE_TAGS = [
        "script", "style", "nav", "header", "footer", 
        "aside", "advertisement", "iframe", "noscript"
    ]
    
    # 常見的雜訊 class 名稱
    NOISE_CLASSES = [
        "ad", "ads", "advertisement", "sidebar", "menu", 
        "navigation", "footer", "header", "social", "share"
    ]

    def __init__(self, min_paragraph_length: int = 20):
        """
        初始化過濾器
        
        Args:
            min_paragraph_length: 最小段落長度（過短的會被過濾）
        """
        self.min_paragraph_length = min_paragraph_length

    def extract(self, html: str) -> FilteredContent:
        """
        從 HTML 提取純文字內容
        
        Args:
            html: 原始 HTML 字串
            
        Returns:
            FilteredContent: 過濾後的內容
        """
        soup = BeautifulSoup(html, "html.parser")
        
        # 移除不需要的標籤
        for tag in self.REMOVE_TAGS:
            for element in soup.find_all(tag):
                element.decompose()
        
        # 移除雜訊元素（根據 class 名稱）
        for class_name in self.NOISE_CLASSES:
            for element in soup.find_all(class_=re.compile(class_name, re.I)):
                element.decompose()
        
        # 提取標題
        title = ""
        title_tag = soup.find("h1") or soup.find("title")
        if title_tag:
            title = title_tag.get_text(strip=True)
        
        # 提取段落
        paragraphs = []
        for p in soup.find_all(["p", "article"]):
            text = p.get_text(strip=True)
            if len(text) >= self.min_paragraph_length:
                paragraphs.append(text)
        
        # 組合正文
        body = "\n\n".join(paragraphs)
        
        return FilteredContent(
            title=title,
            body=body,
            paragraphs=paragraphs,
            word_count=len(body)
        )

    def clean_text(self, text: str) -> str:
        """
        清理文字（移除多餘空白與特殊字元）
        
        Args:
            text: 原始文字
            
        Returns:
            str: 清理後的文字
        """
        # 移除多餘空白
        text = re.sub(r'\s+', ' ', text)
        # 移除特殊控制字元
        text = re.sub(r'[\x00-\x1f\x7f-\x9f]', '', text)
        return text.strip()


# 範例使用
if __name__ == "__main__":
    sample_html = """
    <html>
        <head><title>測試新聞標題</title></head>
        <body>
            <nav>導覽列</nav>
            <h1>這是新聞標題</h1>
            <p>這是第一段正文內容，描述了重要的新聞事件。</p>
            <p>這是第二段正文內容，提供了更多細節資訊。</p>
            <footer>頁尾資訊</footer>
        </body>
    </html>
    """
    
    text_filter = TextFilter()
    result = text_filter.extract(sample_html)
    
    print(f"標題: {result.title}")
    print(f"段落數: {len(result.paragraphs)}")
    print(f"正文:\n{result.body}")
