"""Interval / fixed-time scheduling helpers.

Kept independent of the CLI so features can call these from their own code
if they want to run loops outside the standard CLI flow.
"""

from __future__ import annotations

import re
from datetime import datetime, timedelta


def seconds_until_hhmm(hhmm: str) -> int:
    """Seconds from now until the next HH:MM. Always returns >= 1."""
    m = re.fullmatch(r"(\d{1,2}):(\d{2})", hhmm.strip())
    if not m:
        raise ValueError("schedule-at must be HH:MM")
    hour, minute = int(m.group(1)), int(m.group(2))
    if not (0 <= hour <= 23 and 0 <= minute <= 59):
        raise ValueError("schedule-at must be HH:MM with valid time")
    now = datetime.now()
    target = now.replace(hour=hour, minute=minute, second=0, microsecond=0)
    if target <= now:
        target += timedelta(days=1)
    return max(1, int((target - now).total_seconds()))
