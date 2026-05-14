"""Scrape KWay meeting-room reservations day by day.

Output shape (one JSON file per run, possibly covering many days):
{
  "scraped_at": "...",
  "range_start": "YYYY-MM-DD",
  "range_end":   "YYYY-MM-DD",
  "days": [
    {
      "date": "YYYY-MM-DD",
      "page_url": "...",
      "rooms": [
        {
          "code": "C12",
          "name": "1號會議室(8人)",
          "bookings": [
            {"time_start": "09:00", "time_end": "12:00", "user": "黃若瑀"}
          ]
        }
      ],
      "snapshot_html": "...",
      "screenshot": "..."
    }
  ],
  "warnings": [...]
}
"""

from __future__ import annotations

import argparse
import os
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Any
from urllib.parse import parse_qsl, urlencode, urlparse, urlunparse

from ..session import PortalSession
from ..output import save_snapshot, timestamp
from .base import PortalFeature


DEFAULT_BOOKING_URL = "https://crm.kway.com.tw/portal_login.jsp?menuid=251510"
DEFAULT_LINK_TEXTS = ("預訂會議室", "預定會議室")
DEFAULT_RESULT_CONTAINER = "body"


def _env(name: str, default: str = "") -> str:
    return os.environ.get(name, default).strip()


def _iter_dates(start_iso: str, end_iso: str):
    s = datetime.strptime(start_iso, "%Y-%m-%d").date()
    e = datetime.strptime(end_iso, "%Y-%m-%d").date()
    current = s
    while current <= e:
        yield current.isoformat()
        current += timedelta(days=1)


def _default_month_range() -> tuple[str, str]:
    today = date.today()
    first = today.replace(day=1)
    if first.month == 12:
        nxt = first.replace(year=first.year + 1, month=1)
    else:
        nxt = first.replace(month=first.month + 1)
    last = nxt - timedelta(days=1)
    return first.isoformat(), last.isoformat()


class MeetingRoomsFeature(PortalFeature):
    """Daily scraper for portal.kway.com.tw → 預訂會議室."""

    name = "meeting-rooms"
    description = "Scrape KWay meeting-room reservations as JSON (per-day loop)."

    @classmethod
    def add_arguments(cls, subparser: argparse.ArgumentParser) -> None:
        first, last = _default_month_range()
        subparser.add_argument(
            "--start",
            default=first,
            help="Start date YYYY-MM-DD (default: first of current month)",
        )
        subparser.add_argument(
            "--end",
            default=last,
            help="End date YYYY-MM-DD (default: last of current month)",
        )
        subparser.add_argument(
            "--output",
            default="",
            help="Explicit output JSON path. Default: <output>/meeting-rooms/<range>_<ts>.json",
        )

    async def run(
        self,
        session: PortalSession,
        args: argparse.Namespace,
        *,
        output_dir: Path,
        snapshot_dir: Path,
    ) -> dict[str, Any]:
        _validate(args.start)
        _validate(args.end)

        booking_url = _env("KWAY_MEETING_ROOMS_BOOKING_URL", DEFAULT_BOOKING_URL)
        link_texts = [
            t.strip()
            for t in _env(
                "KWAY_MEETING_ROOMS_LINK_TEXT",
                ",".join(DEFAULT_LINK_TEXTS),
            ).split(",")
            if t.strip()
        ]
        result_container = _env(
            "KWAY_MEETING_ROOMS_RESULT_CONTAINER", DEFAULT_RESULT_CONTAINER
        )

        page = await session.open_portal_link(
            link_text_candidates=link_texts,
            direct_url=booking_url,
        )

        ts = timestamp()
        days: list[dict[str, Any]] = []
        total = 0
        for iso in _iter_dates(args.start, args.end):
            try:
                await _navigate_to_date(session, iso)
                await session.close_announcement_popups(f"after navigate to {iso}")

                try:
                    await page.locator(result_container).first.wait_for(
                        timeout=session.cfg.timeout_ms
                    )
                except Exception:
                    session.warnings.append(
                        f"Result container not found on {iso}: {result_container}"
                    )

                rooms = await _extract_room_bookings(page)
                html_p, png_p = await save_snapshot(
                    page, snapshot_dir, f"meeting_rooms_{iso}_{ts}"
                )
                day_total = sum(len(r["bookings"]) for r in rooms)
                total += day_total
                days.append({
                    "date": iso,
                    "page_url": page.url,
                    "rooms": rooms,
                    "snapshot_html": str(html_p),
                    "screenshot": str(png_p),
                })
                print(f"[ok] {iso}: {len(rooms)} rooms, {day_total} bookings")
            except Exception as exc:
                session.warnings.append(f"Day {iso} failed: {type(exc).__name__}: {exc}")
                days.append({
                    "date": iso,
                    "page_url": page.url if not page.is_closed() else "",
                    "rooms": [],
                    "snapshot_html": "",
                    "screenshot": "",
                    "error": f"{type(exc).__name__}: {exc}",
                })

        return {
            "scraped_at": datetime.now().isoformat(timespec="seconds"),
            "portal_url": session.cfg.portal_url,
            "booking_url": page.url if not page.is_closed() else "",
            "range_start": args.start,
            "range_end": args.end,
            "days": days,
            "warnings": session.warnings,
        }


def _validate(value: str) -> None:
    datetime.strptime(value, "%Y-%m-%d")


async def _navigate_to_date(session: PortalSession, iso_date: str) -> None:
    """Switch the booking page to a specific date.

    Prefer filling the page's setdate input (which triggers gonextday() on
    change); fall back to mutating the URL date param.
    """
    portal_date = iso_date.replace("-", "/")  # "2026/05/13"
    page = session.page
    try:
        await page.locator("#setdate").first.fill(
            portal_date, timeout=session.cfg.timeout_ms
        )
        await page.evaluate(
            "() => { if (typeof gonextday === 'function') gonextday(); }"
        )
        await session.safe_wait_networkidle()
        return
    except Exception as exc:
        session.warnings.append(
            f"setdate fill failed for {iso_date}: {type(exc).__name__}: {exc}; falling back to URL"
        )

    parsed = urlparse(page.url)
    params = dict(parse_qsl(parsed.query))
    params["date"] = portal_date
    params.setdefault("key", "C")
    new_url = urlunparse(parsed._replace(query=urlencode(params)))
    await page.goto(
        new_url, wait_until="domcontentloaded", timeout=session.cfg.timeout_ms
    )
    await session.safe_wait_networkidle()


async def _extract_room_bookings(page) -> list[dict[str, Any]]:
    """Pull booking metadata from the current conferenceListByRoom view.

    Bookings carry all useful info in their preview link href:
      conference_mgr.jsp?key=C&op=preview&cod_conference=C12
        &time_start=0900&time_end=1200&dat_conference=2026/05/13
    The column header has a separate link with `&room=C12` pointing to the
    weekly view, which we use to translate room codes to display names.
    """
    return await page.evaluate(
        r"""
        () => {
          const codeToName = {};
          for (const a of document.querySelectorAll('a[href*="conferenceListByWeek.jsp"]')) {
            const m = (a.getAttribute('href') || '').match(/[?&]room=([^&]+)/);
            if (!m) continue;
            const code = decodeURIComponent(m[1]);
            const name = (a.innerText || '').replace(/\s+/g, ' ').trim();
            if (name) codeToName[code] = name;
          }

          const byCode = {};
          for (const a of document.querySelectorAll('a[href*="conference_mgr.jsp"][href*="op=preview"]')) {
            const href = a.getAttribute('href') || '';
            const q = {};
            for (const pair of href.split('?')[1]?.split('&') || []) {
              const [k, v] = pair.split('=');
              if (k) q[decodeURIComponent(k)] = decodeURIComponent((v || '').replace(/\+/g, ' '));
            }
            const code = q['cod_conference'];
            const ts = q['time_start'];
            const te = q['time_end'];
            if (!code || !ts || !te) continue;
            const fmt = (hhmm) => `${hhmm.slice(0, 2)}:${hhmm.slice(2, 4)}`;
            const user = (a.innerText || '').replace(/\s+/g, ' ').trim();
            if (!byCode[code]) byCode[code] = [];
            byCode[code].push({
              time_start: fmt(ts),
              time_end: fmt(te),
              user,
            });
          }

          const rooms = [];
          for (const code of Object.keys(byCode).sort()) {
            rooms.push({
              code,
              name: codeToName[code] || code,
              bookings: byCode[code].sort((a, b) => a.time_start.localeCompare(b.time_start)),
            });
          }
          return rooms;
        }
        """
    )
