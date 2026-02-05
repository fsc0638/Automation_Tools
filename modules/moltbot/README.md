# Moltbot News Crawler

這是一個基於 Python 的模塊化新聞爬取與判讀系統。

## 模塊結構

| 模塊 | 檔案 | 功能 |
|------|------|------|
| 數據爬蟲 | `crawler.py` | 抓取新聞網頁 |
| 文字過濾器 | `filter.py` | 清理 HTML 提取正文 |
| 關鍵字分析 | `analyzer.py` | 詞頻與實體分析 |
| API 連接器 | `connector.py` | LLM 與通訊平台串接 |

## 安裝

```bash
cd modules/moltbot
pip install -r requirements.txt
playwright install chromium
```

## 使用方式

```bash
python main.py
```
