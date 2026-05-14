"""KWay portal employee-directory scraper (員工通訊錄).

Hits the portal's `sm_key_search()` with an empty keyword (mirrors the
"直接空白搜尋 = all employees" UX), then parses the result table inside
`#sm_search_content`. Writes two date-keyed JSON files per run:

    output/employee-directory/employees_<week-monday>.json
    output/employee-directory/departments_<week-monday>.json

`week-monday` is the Monday of the run week, so re-running the same week
overwrites the previous pair; the directory grows by one pair per week.

The full employee row layout per the May 2026 portal version is:
    [#, "<no> <name>", "<ext>[,<ext>...]", "<dept_code> <dept_name>", title, email]
Detection lives in `_extract_employees`; if KWay shifts the markup the
JS payload should fail fast (zero rows → caller raises) so the backend
importer's safety net skips the empty result rather than nuking the DB.
"""

from __future__ import annotations

import argparse
import os
import re
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Any

from ..output import save_snapshot, write_json
from ..session import PortalSession
from .base import PortalFeature

DEFAULT_LINK_TEXTS = ("員工通訊錄",)
DEFAULT_DIRECT_URL = "https://portal.kway.com.tw/main/search_member.aspx"


def _env(name: str, default: str = "") -> str:
    return os.environ.get(name, default).strip()


def _monday_of(d: date) -> date:
    return d - timedelta(days=d.weekday())


class EmployeeDirectoryFeature(PortalFeature):
    """Weekly scrape of the KWay portal employee directory."""

    name = "employee-directory"
    description = "Scrape KWay portal employee directory + department list."

    @classmethod
    def add_arguments(cls, subparser: argparse.ArgumentParser) -> None:
        subparser.add_argument(
            "--output",
            default="",
            help=(
                "Explicit base path for the employees JSON. If empty, defaults"
                " to <output>/employee-directory/employees_<week-monday>.json,"
                " and the departments file lives alongside it."
            ),
        )

    async def run(
        self,
        session: PortalSession,
        args: argparse.Namespace,
        *,
        output_dir: Path,
        snapshot_dir: Path,
    ) -> dict[str, Any]:
        link_texts = [
            t.strip()
            for t in _env(
                "KWAY_EMPLOYEE_DIRECTORY_LINK_TEXT",
                ",".join(DEFAULT_LINK_TEXTS),
            ).split(",")
            if t.strip()
        ]
        direct_url = _env("KWAY_EMPLOYEE_DIRECTORY_URL", DEFAULT_DIRECT_URL)

        page = await session.open_portal_link(
            link_text_candidates=link_texts,
            direct_url=direct_url,
        )
        await session.safe_wait_networkidle()
        await session.close_announcement_popups("after employee-directory open")

        # The portal's keyword-search box is wired to `sm_key_search()` via
        # an Enter-key handler. Calling it with an empty key returns every
        # employee — exactly the behaviour the user wants for the weekly
        # snapshot.
        try:
            await page.evaluate(
                r"""
                () => {
                  const key = document.querySelector('#sm_search_key');
                  if (key) key.value = '';
                  if (typeof sm_key_search === 'function') sm_key_search();
                }
                """
            )
            try:
                await page.locator("#sm_search_content tr.HRN").first.wait_for(
                    timeout=session.cfg.timeout_ms
                )
            except Exception:
                # No result rows appeared. Could be a legitimate empty list,
                # could be a search failure — either way the extractor below
                # will return zero employees and the caller's guard kicks in.
                session.warnings.append("No result rows found in #sm_search_content")
            await session.safe_wait_networkidle()
        except Exception as exc:
            session.warnings.append(
                f"sm_key_search invocation failed: {type(exc).__name__}: {exc}"
            )

        # Health check: page must have the dept dropdown + a result table.
        # If neither is present we lost the session or the URL changed —
        # raise so the outer try/except (in cli) records a failure and the
        # importer (when added) skips the day rather than wiping DB rows.
        health = await page.evaluate(
            r"""
            () => ({
              hasDeptSelect:
                document.querySelectorAll('#sm_dept_id1 option').length > 0,
              hasResultContainer:
                !!document.querySelector('#sm_search_content'),
            })
            """
        )
        if not health.get("hasDeptSelect") or not health.get("hasResultContainer"):
            raise RuntimeError(
                "employee-directory page didn't render expected widgets "
                f"(hasDeptSelect={health.get('hasDeptSelect')}, "
                f"hasResultContainer={health.get('hasResultContainer')}); "
                "likely lost session — try --headed --keep-signed-in"
            )

        departments = await _extract_departments(page)
        employees = await _extract_employees(page)

        week_monday = _monday_of(date.today()).isoformat()

        # Re-point the CLI's default-write to employees_<monday>.json so
        # we don't end up with both an `employee-directory_<run-day>.json`
        # AND the `employees_<monday>.json` we actually want. The CLI
        # respects args.output as the destination of the value returned
        # by run().
        if not getattr(args, "output", ""):
            args.output = str(output_dir / f"employees_{week_monday}.json")

        # Side-effect: drop the departments side-file. CLI doesn't know
        # about it, so we write directly here.
        dept_payload = {
            "scraped_at": datetime.now().isoformat(timespec="seconds"),
            "week_starting": week_monday,
            "total": len(departments),
            "departments": departments,
        }
        dept_path = output_dir / f"departments_{week_monday}.json"
        write_json(dept_path, dept_payload)

        html_p, png_p = await save_snapshot(
            page, snapshot_dir, f"employee_directory_{week_monday}"
        )

        return {
            "scraped_at": datetime.now().isoformat(timespec="seconds"),
            "week_starting": week_monday,
            "total": len(employees),
            "employees": employees,
            "departments_path": str(dept_path),
            "snapshot_html": str(html_p),
            "screenshot": str(png_p),
            "warnings": session.warnings,
        }


async def _extract_departments(page) -> list[dict[str, str]]:
    """Pull (code, name) pairs from the dept dropdown #sm_dept_id1.

    Options are formatted "<code>　<name>" (note: full-width space).
    The first option is "全部" and gets skipped.
    """
    return await page.evaluate(
        r"""
        () => {
          const sel = document.querySelector('#sm_dept_id1');
          if (!sel) return [];
          return Array.from(sel.options)
            .filter(o => o.value && o.value.trim() !== '')
            .map(o => {
              const text = (o.text || '').trim();
              // Split on the first whitespace (regular OR full-width).
              const m = text.match(/^(\S+)[\s　]+(.+)$/);
              return m
                ? { code: m[1].trim(), name: m[2].trim() }
                : { code: o.value, name: text };
            });
        }
        """
    )


async def _extract_employees(page) -> list[dict[str, Any]]:
    """Read the result rows from #sm_search_content.

    Layout:
        cell 0  序號 (skip)
        cell 1  "<employee_no> <name>"
        cell 2  分機 — single value or comma-separated, e.g. "101,102"
        cell 3  "<dept_code> <dept_name>"
        cell 4  職稱
        cell 5  信箱
    Rows are tagged with class="HRN" inside #sm_search_content.
    """
    raw_rows: list[list[str]] = await page.evaluate(
        r"""
        () => {
          const container = document.querySelector('#sm_search_content');
          if (!container) return [];
          return Array.from(container.querySelectorAll('tr.HRN')).map(tr =>
            Array.from(tr.querySelectorAll('td')).map(
              td => (td.innerText || '').replace(/\s+/g, ' ').trim()
            )
          );
        }
        """
    )

    employees: list[dict[str, Any]] = []
    for cells in raw_rows:
        if len(cells) < 6:
            continue
        name_cell = cells[1]
        dept_cell = cells[3]
        ext_cell = cells[2]
        title_cell = cells[4]
        email_cell = cells[5]

        # "1668 王崇旭" → ("1668", "王崇旚")
        m_name = re.match(r"^(\S+)\s+(.+)$", name_cell)
        if m_name:
            employee_no, name = m_name.group(1), m_name.group(2)
        else:
            employee_no, name = "", name_cell

        # "A000 董事長室" → ("A000", "董事長室")
        m_dept = re.match(r"^(\S+)\s+(.+)$", dept_cell)
        if m_dept:
            dept_code, dept_name = m_dept.group(1), m_dept.group(2)
        else:
            dept_code, dept_name = "", dept_cell

        # Extensions: split on comma + whitespace, drop empties.
        extensions = [
            tok.strip()
            for tok in re.split(r"[,\s]+", ext_cell)
            if tok.strip()
        ]

        employees.append({
            "employee_no": employee_no,
            "name": name,
            "extensions": extensions,
            "dept_code": dept_code,
            "dept_name": dept_name,
            "title": title_cell,
            "email": email_cell,
        })
    return employees
