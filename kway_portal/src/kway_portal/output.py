"""File-output helpers.

Each feature writes to:
  <output_root>/<feature_name>/<filename>.json
  <snapshot_root>/<feature_name>/<filename>.html | .png

This keeps results from different features from colliding.
"""

from __future__ import annotations

import json
from datetime import datetime
from pathlib import Path
from typing import Any

from playwright.async_api import Page


def timestamp() -> str:
    return datetime.now().strftime("%Y%m%d_%H%M%S")


def feature_dir(root: Path, feature_name: str) -> Path:
    d = root / feature_name
    d.mkdir(parents=True, exist_ok=True)
    return d


def write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(data, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )


async def save_snapshot(
    page: Page,
    snapshot_dir: Path,
    base_name: str,
) -> tuple[Path, Path]:
    """Save full-page HTML + full-page screenshot. Returns the two paths."""
    snapshot_dir.mkdir(parents=True, exist_ok=True)
    html_path = snapshot_dir / f"{base_name}.html"
    screenshot_path = snapshot_dir / f"{base_name}.png"
    html_path.write_text(await page.content(), encoding="utf-8")
    await page.screenshot(path=str(screenshot_path), full_page=True)
    return html_path, screenshot_path
