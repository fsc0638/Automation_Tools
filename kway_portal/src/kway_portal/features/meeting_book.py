"""Write-side counterpart of meeting_rooms: submit / cancel reservations.

Three operations, selected via --op:

  --op book        single-day  → POSTs conference_mgr.jsp
  --op book-multi  weekly      → POSTs conference_mgrM.jsp (週期 = N)
  --op cancel      window      → POSTs conference_mgrD.jsp

Output shape (always one JSON to stdout AND saved to disk so the backend
subprocess wrapper can parse `success`/`error` without scraping logs):

  {
    "op": "book" | "book-multi" | "cancel",
    "success": true|false,
    "room_code": "C11",
    "date": "2026-05-14",            # single-day
    "date_start": "2026-05-14",       # multi / cancel
    "date_end":   "2026-06-14",       # multi / cancel
    "period_weeks": 1,                # multi / cancel
    "time_start": "08:00",
    "time_end":   "09:00",
    "subject":    "...",              # book / book-multi only
    "submitted_url": "...",
    "result_text": "...short tail of the response page",
    "snapshot_html": "...",
    "screenshot": "...",
    "error": "..."                    # present iff !success
  }

The portal sometimes returns an HTML page with a Chinese error string
("時段已被預約", "重複預約", "時間錯誤" …). We treat any of those tokens as
a failure so the backend marks the meeting `status='draft'`. A response
that contains 「成功」or 「已新增」 is treated as success; everything else
is success-on-best-guess (no error tokens → we believe the portal). The
backend can re-classify in the SyncReport prefix string `[ok]` / `[err]`.
"""

from __future__ import annotations

import argparse
import json
from datetime import datetime
from pathlib import Path
from typing import Any

from ..session import PortalSession
from ..output import save_snapshot
from .base import PortalFeature


BOOK_URL = "https://crm.kway.com.tw/cgi/conference/conference_mgr.jsp?key=C"
BOOK_MULTI_URL = "https://crm.kway.com.tw/cgi/conference/conference_mgrM.jsp?key=C"
CANCEL_URL = "https://crm.kway.com.tw/cgi/conference/conference_mgrD.jsp?key=C"

# Booking list page; same one the meeting_rooms scraper uses. The portal's
# fn_open() refuses to take a direct conference_mgr.jsp URL — it punts back
# to main_new.jsp — so we *first* land on the list page (menuid=251510)
# inside the portal frame, *then* navigate to the form by page.goto() which
# does keep session cookies because we're already on the correct host.
BOOKING_LIST_URL = "https://crm.kway.com.tw/portal_login.jsp?menuid=251510"

ERROR_TOKENS = (
    "已被預約",
    "重複預約",
    "時間錯誤",
    "錯誤",
    "請重新",
    "失敗",
    "no permission",
    "Error",
)
SUCCESS_TOKENS = ("成功", "已新增", "完成", "已取消", "已刪除")


def _to_portal_date(iso: str) -> str:
    """2026-05-14 → 2026/05/14 (portal form expects slashes)."""
    return iso.replace("-", "/")


def _to_hhmm_parts(hhmm: str) -> tuple[str, str]:
    """\"08:30\" → (\"08\", \"30\")."""
    h, m = hhmm.split(":")
    return h.zfill(2), m.zfill(2)


def _classify(html_tail: str) -> tuple[bool, str]:
    """Return (success, error_message). Order: error tokens > success > assume-ok."""
    for tok in ERROR_TOKENS:
        if tok in html_tail:
            return False, f"portal reported: {tok}"
    for tok in SUCCESS_TOKENS:
        if tok in html_tail:
            return True, ""
    # Best-guess fallback. The portal's success pages tend to redirect back
    # to the listing without a body marker, so absence of error tokens is
    # usually enough.
    return True, ""


class MeetingBookFeature(PortalFeature):
    name = "meeting-book"
    description = "Book or cancel a meeting-room reservation on the KWay portal."

    @classmethod
    def add_arguments(cls, sp: argparse.ArgumentParser) -> None:
        sp.add_argument(
            "--op",
            choices=["book", "book-multi", "cancel"],
            required=True,
            help="Which form to drive: single-day book / weekly book / cancel-window.",
        )
        sp.add_argument("--room-code", required=True, help="Portal room code, e.g. C11.")
        sp.add_argument(
            "--room-name",
            default="",
            help="Optional room display name; used if the <select> doesn't expose codes.",
        )
        sp.add_argument("--date", default="", help="(book) ISO date YYYY-MM-DD.")
        sp.add_argument("--date-start", default="", help="(book-multi/cancel) range start.")
        sp.add_argument("--date-end", default="", help="(book-multi/cancel) range end.")
        sp.add_argument(
            "--period-weeks",
            type=int,
            default=1,
            help="(book-multi/cancel) weekly recurrence interval; portal default = 1.",
        )
        sp.add_argument("--time-start", required=True, help="HH:MM (24h).")
        sp.add_argument("--time-end", required=True, help="HH:MM (24h).")
        sp.add_argument("--subject", default="", help="(book/book-multi) 說明 / purpose text.")
        sp.add_argument(
            "--output",
            default="",
            help="Override output JSON path. Defaults to "
            "output/meeting-book/<op>_<date>_<room>.json.",
        )

    async def run(
        self,
        session: PortalSession,
        args: argparse.Namespace,
        *,
        output_dir: Path,
        snapshot_dir: Path,
    ) -> dict[str, Any]:
        op = args.op
        target = {
            "book": BOOK_URL,
            "book-multi": BOOK_MULTI_URL,
            "cancel": CANCEL_URL,
        }[op]

        # Two-step navigation. fn_open() refuses arbitrary URLs (it punts
        # back to main_new.jsp), so:
        #   1. Open the booking list via the portal menu (menuid=251510).
        #      This is exactly the page the meeting_rooms scraper uses.
        #   2. From that page, page.goto() the specific form URL. By this
        #      point we share the booking-list frame's host + session
        #      cookies, so the form actually loads.
        page = await session.open_portal_link(
            link_text_candidates=[],
            direct_url=BOOKING_LIST_URL,
        )
        await session.safe_wait_networkidle()
        await session.close_announcement_popups(f"after open list ({op})")
        await page.goto(target, wait_until="domcontentloaded",
                        timeout=session.cfg.timeout_ms)
        await session.safe_wait_networkidle()
        await session.close_announcement_popups(f"after open form ({op})")

        # The portal sometimes lazy-loads the room <select> options after
        # initial DOMContentLoaded. Wait until at least one <select> in the
        # form has > 1 option before trying to pick a room. Short timeout
        # because if it's still empty after 5s, something else is wrong.
        try:
            await page.wait_for_function(
                "() => Array.from(document.querySelectorAll('form select'))"
                ".some(s => s.options.length > 1)",
                timeout=5000,
            )
        except Exception:
            session.warnings.append(
                f"{op}: no populated <select> on form within 5s; "
                "proceeding anyway — _pick_room will surface the diagnosis."
            )

        # Pick the room. Two strategies: match <option value="..."> against
        # --room-code first; if no <option> has that value, fall back to
        # label match against --room-name. The select element name varies
        # by form; we try the common KWay names in order.
        chosen = await _pick_room(page, args.room_code, args.room_name)
        if chosen is None:
            # Dump *what we actually saw* — current URL, every <select>'s
            # name + first 30 options. Previous "room not found" message
            # gave zero clue whether we'd hit the login page, an empty
            # form, or a form with a select whose label format differs.
            page_diag = await page.evaluate(
                """
                () => {
                  const selects = Array.from(document.querySelectorAll('select')).map(s => ({
                    name: s.name || s.id || '(unnamed)',
                    option_count: s.options.length,
                    sample_options: Array.from(s.options).slice(0, 30).map(o => ({
                      value: o.value, text: (o.text || '').trim()
                    })),
                  }));
                  return {
                    url: location.href,
                    title: document.title,
                    has_form: !!document.querySelector('form'),
                    select_count: selects.length,
                    selects,
                  };
                }
                """
            )
            html_p, png_p = await save_snapshot(
                page,
                snapshot_dir,
                f"{op}_no_room_{datetime.now().strftime('%Y%m%d_%H%M%S')}",
            )
            return {
                "op": op,
                "success": False,
                "room_code": args.room_code,
                "room_name": args.room_name,
                "page_diagnostic": page_diag,
                "date": args.date,
                "date_start": args.date_start,
                "date_end": args.date_end,
                "time_start": args.time_start,
                "time_end": args.time_end,
                "error": f"room {args.room_code} ({args.room_name}) not found in 預約項目 dropdown",
                "snapshot_html": str(html_p),
                "screenshot": str(png_p),
            }

        # Before filling, snapshot every named field on the form. This is
        # how we discover the actual time / period / subject field names
        # for the first time — selector lists in _fill_form are guesses
        # against KWay's naming convention, and "portal reported: 時間
        # 錯誤" with no filled time means the time guess missed.
        form_inputs = await page.evaluate(
            """
            () => {
              const out = [];
              for (const el of document.querySelectorAll('form input, form select, form textarea')) {
                const item = {
                  tag: el.tagName.toLowerCase(),
                  type: el.type || '',
                  name: el.name || '',
                  id: el.id || '',
                  readonly: el.hasAttribute('readonly') || !!el.readOnly,
                  value: el.value ?? '',
                };
                if (el.tagName === 'SELECT') {
                  item.option_count = el.options.length;
                  item.sample_options = Array.from(el.options).slice(0, 8)
                    .map(o => ({value: o.value, text: (o.text || '').trim()}));
                }
                out.push(item);
              }
              return out;
            }
            """
        )

        # Fill the date / time / 說明 fields. Each helper is tolerant of
        # missing fields and reports back what it actually filled so the
        # caller can decide if the form is workable.
        filled = await _fill_form(page, op, args)

        # Submit. Most KWay forms have a <input type="submit" value="確定">
        # with onclick=mpm_ml_box3_*; both routes work — Playwright click
        # on the visible 確定 button is the most reliable.
        submitted_url = await _submit(page, session)

        # Read a tail of the response page for classification + snapshotting.
        body_text = (await page.content())[-2000:]
        success, error = _classify(body_text)

        html_p, png_p = await save_snapshot(
            page,
            snapshot_dir,
            f"{op}_{datetime.now().strftime('%Y%m%d_%H%M%S')}_{args.room_code}",
        )

        out = {
            "op": op,
            "success": success,
            "room_code": args.room_code,
            "room_name": chosen,
            "date": args.date,
            "date_start": args.date_start,
            "date_end": args.date_end,
            "period_weeks": args.period_weeks,
            "time_start": args.time_start,
            "time_end": args.time_end,
            "subject": args.subject,
            "submitted_url": submitted_url,
            "filled_fields": filled,
            "form_inputs": form_inputs,
            "result_text": body_text[-800:],
            "snapshot_html": str(html_p),
            "screenshot": str(png_p),
        }
        if error:
            out["error"] = error
        return out


async def _pick_room(page, room_code: str, room_name: str) -> str | None:
    """Find the room <select> and choose the matching option.

    Returns the option label that was selected, or None when we couldn't
    find a match. The portal's room selects don't have a stable element
    name across forms, so we try the common ones plus a generic fallback
    that looks for any <select> whose options include the target code or
    name. The fallback is what saves us when the portal renames a field.
    """
    candidates = [
        'select[name="cod_conference"]',
        'select[name="conference"]',
        'select[name="room"]',
    ]
    for sel in candidates:
        loc = page.locator(sel).first
        if await loc.count() == 0:
            continue
        return await _select_option(loc, room_code, room_name)

    # Generic fallback: any <select> in the form.
    all_selects = page.locator("form select")
    n = await all_selects.count()
    for i in range(n):
        loc = all_selects.nth(i)
        label = await _select_option(loc, room_code, room_name)
        if label:
            return label
    return None


async def _select_option(loc, room_code: str, room_name: str) -> str | None:
    # Prefer code match (deterministic), fall back to label match (lenient).
    options = await loc.evaluate(
        "(el) => Array.from(el.options).map(o => ({value: o.value, text: o.text.trim()}))"
    )
    # IMPORTANT: skip code matching when room_code is empty — `"C18".endswith("")`
    # returns True, which would silently pick the *first* option (台中大會議室
    # on conference_mgr.jsp) regardless of the actual room the user chose.
    # The 2026-05-15 first-success run hit exactly this and booked the
    # wrong room.
    if room_code:
        for o in options:
            if o["value"] == room_code or o["value"].endswith(room_code):
                await loc.select_option(value=o["value"])
                return o["text"]
    if room_name:
        # Bidirectional substring: portal may shorten ("6號會議室" vs our
        # "6號會議室(8人)") or pad ("6號會議室 - 1F"). Either direction is
        # fine because the room dropdown values are unambiguous within
        # one workspace.
        rn = room_name.strip()
        for o in options:
            text = o["text"].strip()
            if rn and (rn in text or text in rn):
                await loc.select_option(value=o["value"])
                return text
    return None


async def _fill_input(loc, value: str) -> None:
    """Fill an input that may be readonly.

    Many KWay forms put `readonly="true"` on date fields so the user has to
    use the custom calendar picker (onclick="new Calendar(...).show(this)").
    Playwright's `fill()` respects readonly and refuses to type — so for
    those inputs we set `value` via DOM and dispatch the input/change
    events the form's onchange handlers expect to see. Non-readonly inputs
    take the normal fill() path so we don't lose Playwright's input
    masking / IME handling.
    """
    is_readonly = await loc.evaluate(
        "(el) => el.hasAttribute('readonly') || el.readOnly"
    )
    if is_readonly:
        await loc.evaluate(
            """(el, v) => {
                el.value = v;
                el.dispatchEvent(new Event('input', {bubbles: true}));
                el.dispatchEvent(new Event('change', {bubbles: true}));
            }""",
            value,
        )
    else:
        await loc.fill(value)


async def _fill_form(page, op: str, args) -> dict[str, str]:
    """Fill date / time / 說明 fields; return what we set for diagnostics."""
    filled: dict[str, str] = {}

    if op == "book":
        portal_date = _to_portal_date(args.date)
        for sel in (
            'input[name="dat_conference"]',
            'input[name="dat"]',
            'input[name="date"]',
        ):
            loc = page.locator(sel).first
            if await loc.count():
                await _fill_input(loc, portal_date)
                filled["date"] = portal_date
                break
    else:
        # KWay's cancel form (conference_mgrD.jsp) names — verified
        # 2026-05-15 via a form_inputs dump:
        #   from-date input = dat_conference (yes, same as single-day book)
        #   to-date input   = dat_conference_end
        #   recurrence      = week_count
        # The book-multi form (conference_mgrM.jsp) hasn't been form-
        # dumped yet; we keep the older guesses as fallbacks so a future
        # multi-book test surfaces what's actually there without 500ing.
        d1 = _to_portal_date(args.date_start)
        d2 = _to_portal_date(args.date_end)
        for sel in (
            'input[name="dat_conference"]',       # cancel form
            'input[name="dat_conference1"]',      # speculative for multi
            'input[name="dat_start"]',
            'input[name="date_start"]',
        ):
            loc = page.locator(sel).first
            if await loc.count():
                await _fill_input(loc, d1)
                filled["date_start"] = d1
                break
        for sel in (
            'input[name="dat_conference_end"]',   # cancel form
            'input[name="dat_conference2"]',      # speculative for multi
            'input[name="dat_end"]',
            'input[name="date_end"]',
        ):
            loc = page.locator(sel).first
            if await loc.count():
                await _fill_input(loc, d2)
                filled["date_end"] = d2
                break
        # 週期 — cancel uses `week_count`; multi may use a different name.
        for sel in (
            'input[name="week_count"]',
            'input[name="period"]',
            'input[name="week_period"]',
            'input[name="cycle"]',
        ):
            loc = page.locator(sel).first
            if await loc.count():
                await _fill_input(loc, str(args.period_weeks))
                filled["period_weeks"] = str(args.period_weeks)
                break

    # Start / end time. KWay's conference_mgr.jsp uses the names
    # `time_start` (hour) + `time_start2` (minute) — confirmed by a
    # form-inputs dump on 2026-05-15. Minute select only has options
    # "00" / "30", so anything else gets rounded down to the nearest 30
    # before submit (we leave that to the caller for now and just match
    # an exact value).
    sh, sm = _to_hhmm_parts(args.time_start)
    eh, em = _to_hhmm_parts(args.time_end)
    for sel, val, key in (
        ('select[name="time_start"]',  sh, "time_start_h"),
        ('select[name="time_start2"]', sm, "time_start_m"),
        ('select[name="time_end"]',    eh, "time_end_h"),
        ('select[name="time_end2"]',   em, "time_end_m"),
    ):
        loc = page.locator(sel).first
        if await loc.count():
            try:
                await loc.select_option(value=val)
                filled[key] = val
            except Exception:
                # Defensive: if the zero-padded value isn't there, try the
                # unpadded form. The portal's dump showed padded values
                # ("08", "09") so this should never trigger, but it costs
                # nothing to keep as a safety net.
                await loc.select_option(value=val.lstrip("0") or "0")
                filled[key] = val.lstrip("0") or "0"

    if op in ("book", "book-multi") and args.subject:
        for sel in (
            'textarea[name="txt_note"]',
            'textarea[name="note"]',
            'textarea[name="description"]',
            'textarea[name="说明"]',
        ):
            loc = page.locator(sel).first
            if await loc.count():
                await _fill_input(loc, args.subject)
                filled["subject"] = args.subject
                break

    return filled


async def _submit(page, session) -> str:
    """Click 確定; wait for navigation; return final URL."""
    candidates = [
        'input[type="submit"][value="確定"]',
        'button:has-text("確定")',
        'input[type="button"][value="確定"]',
    ]
    for sel in candidates:
        loc = page.locator(sel).first
        if await loc.count():
            try:
                async with page.expect_navigation(
                    wait_until="domcontentloaded",
                    timeout=session.cfg.timeout_ms,
                ):
                    await loc.click()
            except Exception:
                # Some KWay buttons submit via onclick handler without a
                # navigation event; fall back to clicking + waiting on
                # network idle.
                await loc.click()
                await session.safe_wait_networkidle()
            return page.url
    raise RuntimeError("確定 button not found on the form")


