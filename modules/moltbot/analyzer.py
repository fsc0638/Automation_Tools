"""
Moltbot Keyword Analyzer Module
關鍵字分析模塊 - 提取關鍵字與詞頻分析
"""

import re
from typing import Dict, List, Tuple
from dataclasses import dataclass
from collections import Counter

# 嘗試導入 jieba，若失敗則使用簡單分詞
try:
    import jieba
    JIEBA_AVAILABLE = True
except ImportError:
    JIEBA_AVAILABLE = False


@dataclass
class AnalysisResult:
    """分析結果結構"""
    keywords: List[Tuple[str, int]]  # (關鍵字, 出現次數)
    total_words: int
    unique_words: int
    top_entities: List[str]


class KeywordAnalyzer:
    """關鍵字分析器 - 支援中日英文"""
    
    # 停用詞列表（常見但無意義的詞）
    STOP_WORDS = {
        # 中文
        "的", "了", "是", "在", "我", "有", "和", "就", "不", "人", "都", "一", "一個",
        "上", "也", "很", "到", "說", "要", "去", "你", "會", "著", "沒有", "看", "好",
        "這", "那", "對", "能", "她", "他", "它", "們", "為", "與", "等", "被", "讓",
        # 日文
        "の", "に", "は", "を", "た", "が", "で", "て", "と", "し", "れ", "さ", "ある",
        "いる", "も", "する", "から", "な", "こと", "として", "い", "や", "など",
        # 英文
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "must", "shall", "can", "need", "dare",
        "to", "of", "in", "for", "on", "with", "at", "by", "from", "as",
        "into", "through", "during", "before", "after", "above", "below",
        "and", "but", "or", "nor", "so", "yet", "both", "either", "neither",
        "this", "that", "these", "those", "i", "you", "he", "she", "it", "we", "they"
    }

    def __init__(self, top_n: int = 20, min_word_length: int = 2):
        """
        初始化分析器
        
        Args:
            top_n: 返回前 N 個關鍵字
            min_word_length: 最小詞彙長度
        """
        self.top_n = top_n
        self.min_word_length = min_word_length

    def tokenize(self, text: str) -> List[str]:
        """
        分詞處理
        
        Args:
            text: 輸入文字
            
        Returns:
            List[str]: 詞彙列表
        """
        if JIEBA_AVAILABLE:
            # 使用 jieba 進行中文分詞
            words = list(jieba.cut(text))
        else:
            # 簡單分詞：按空白與標點切割
            words = re.findall(r'[\u4e00-\u9fff]+|[a-zA-Z]+|\d+', text)
        
        # 過濾停用詞與過短詞彙
        filtered = [
            w.lower() for w in words 
            if len(w) >= self.min_word_length and w.lower() not in self.STOP_WORDS
        ]
        return filtered

    def analyze(self, text: str) -> AnalysisResult:
        """
        分析文字並提取關鍵字
        
        Args:
            text: 輸入文字
            
        Returns:
            AnalysisResult: 分析結果
        """
        words = self.tokenize(text)
        word_counts = Counter(words)
        
        # 取得前 N 個高頻詞
        top_keywords = word_counts.most_common(self.top_n)
        
        # 嘗試識別實體（簡單規則：連續大寫開頭的詞）
        entities = self._extract_entities(text)
        
        return AnalysisResult(
            keywords=top_keywords,
            total_words=len(words),
            unique_words=len(word_counts),
            top_entities=entities[:10]
        )

    def _extract_entities(self, text: str) -> List[str]:
        """
        簡單實體識別（公司名、人名等）
        
        Args:
            text: 輸入文字
            
        Returns:
            List[str]: 識別到的實體
        """
        # 匹配可能的實體模式
        patterns = [
            r'[A-Z][a-z]+(?:\s+[A-Z][a-z]+)+',  # 英文名稱
            r'[\u4e00-\u9fff]{2,4}(?:公司|集團|銀行|企業)',  # 中文公司
            r'[\u4e00-\u9fff]{2,3}(?:先生|女士|總裁|董事)',  # 中文人名
        ]
        
        entities = []
        for pattern in patterns:
            matches = re.findall(pattern, text)
            entities.extend(matches)
        
        return list(set(entities))


# 範例使用
if __name__ == "__main__":
    sample_text = """
    台積電今日宣布將在日本熊本縣建設第二座晶片廠。
    根據報導，這座新工廠預計投資約200億美元。
    台積電董事長劉德音表示，這項投資將進一步強化供應鏈。
    日本政府對此表示歡迎，首相岸田文雄親自出席記者會。
    """
    
    analyzer = KeywordAnalyzer()
    result = analyzer.analyze(sample_text)
    
    print(f"總詞數: {result.total_words}")
    print(f"獨立詞數: {result.unique_words}")
    print(f"\n前 10 關鍵字:")
    for word, count in result.keywords[:10]:
        print(f"  {word}: {count}")
    print(f"\n識別實體: {result.top_entities}")
