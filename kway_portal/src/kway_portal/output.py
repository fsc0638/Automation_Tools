"""File-output helpers.

Each feature writes to:
  <output_root>/<feature_name>/<filename>.json
  <snapshot_root>/<feature_name>/<filename>.html | .png

This keeps results from different features from colliding.
"""

from __future__ import annotations

import json
import re
from datetime import datetime
from pathlib import Path
from typing import Any

from playwright.async_api import Page


def timestamp() -> str:
    return datetime.now().strftime("%Y%m%d_%H%M%S")


# Matches the legacy filename suffix from the old "per-run timestamp" naming
# scheme (e.g. "..._20260514_115025.json", "..._20260514_115025.html"). The
# current scheme keys files by calendar date only and overwrites in place,
# so these stale files just accumulate — purge_legacy_timestamped clears
# them out so the output / snapshot directories don't grow forever.
_LEGACY_TIMESTAMP_RE = re.compile(r"_\d{8}_\d{6}\.[A-Za-z0-9]+$")


def feature_dir(root: Path, feature_name: str) -> Path:
    d = root / feature_name
    d.mkdir(parents=True, exist_ok=True)
    return d


def purge_legacy_timestamped(directory: Path) -> int:
    """Delete files in `directory` that match the legacy timestamp suffix.

    Returns count deleted. Safe to call repeatedly; non-matching files
    (including the new date-keyed ones, .browser-profile, etc.) are left
    alone. Permission/race errors are swallowed so a stale lock on one
    file doesn't abort the whole prune.
    """
    if not directory.is_dir():
        return 0
    n = 0
    for p in directory.iterdir():
        if not p.is_file():
            continue
        if _LEGACY_TIMESTAMP_RE.search(p.name):
            try:
                p.unlink()
                n += 1
            except OSError:
                pass
    return n


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
