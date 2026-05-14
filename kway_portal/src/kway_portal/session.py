"""PortalSession: authenticated browser session against portal.kway.com.tw.

Each feature gets a PortalSession instance and uses its helpers to:
- Reach the portal home page (login or reuse persistent profile)
- Open a sub-menu/page either by clicking a menu text or by calling the
  portal's `fn_open()` helper with a direct URL
- Dismiss announcement modals and stray popup windows the way a human would

The actual scraping/automation logic lives in feature modules.
"""

from __future__ import annotations

import asyncio
import re
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from playwright.async_api import (
    BrowserContext,
    Page,
    TimeoutError as PlaywrightTimeoutError,
)

from .browser import BrowserHandle, launch_browser
from .config import PortalConfig


@dataclass
class PortalSession:
    """Authenticated session bound to one Playwright BrowserContext.

    Use `await PortalSession.open(cfg, …)` to construct. Features then call
    `session.open_portal_link(...)` to reach the page they care about.
    """

    cfg: PortalConfig
    context: BrowserContext
    page: Page
    warnings: list[str]
    _handle: BrowserHandle

    # ---- construction / teardown -----------------------------------------

    @classmethod
    async def open(
        cls,
        cfg: PortalConfig,
        *,
        headed: bool = False,
        slow_mo: int = 0,
        keep_signed_in: bool = False,
        manual_login_timeout: int = 0,
        profile_dir: Path | None = None,
    ) -> "PortalSession":
        if profile_dir is None:
            profile_dir = Path.cwd() / ".browser-profile"
        handle = await launch_browser(
            headed=headed,
            slow_mo=slow_mo,
            keep_signed_in=keep_signed_in,
            profile_dir=profile_dir,
        )
        session = cls(
            cfg=cfg,
            context=handle.context,
            page=handle.page,
            warnings=[],
            _handle=handle,
        )
        await session._install_context_handlers()
        await session._login_if_needed(manual_login_timeout)
        return session

    async def close(self) -> None:
        await self._handle.close()

    # ---- public helpers (used by features) -------------------------------

    async def safe_wait_networkidle(self, page: Page | None = None) -> None:
        target = page or self.page
        if target.is_closed():
            return
        try:
            await target.wait_for_load_state("networkidle", timeout=self.cfg.timeout_ms)
        except PlaywrightTimeoutError:
            # ASP.NET portals often keep long-polling requests open. Treat
            # this as "best effort, move on".
            pass
        except Exception:
            pass

    async def active_page(self, preferred: Page | None = None) -> Page:
        candidate = preferred or self.page
        if candidate and not candidate.is_closed():
            return candidate
        for p in reversed(self.context.pages):
            if not p.is_closed():
                self.page = p
                return p
        self.page = await self.context.new_page()
        return self.page

    async def close_announcement_popups(self, stage: str) -> Page:
        """Close in-page modals + safe-to-close extra announcement windows."""
        page = await self.active_page(self.page)

        # KWay homepage news modal: usually only has a checkbox + click-outside.
        try:
            news_modal = page.locator(
                ".modal-content:has(#mpm_news_info_desc), .modal-content:has(#mpm_news_check)"
            ).first
            if await news_modal.count() and await news_modal.is_visible(timeout=500):
                try:
                    checkbox = page.locator("#mpm_news_check").first
                    if await checkbox.count() and await checkbox.is_visible(timeout=300):
                        await checkbox.check(timeout=1200, force=True)
                        self.warnings.append(
                            f"Closed KWay news modal at {stage} via #mpm_news_check"
                        )
                        await asyncio.sleep(0.3)
                except Exception:
                    pass

                if await news_modal.is_visible(timeout=300):
                    await page.mouse.click(8, 8)
                    self.warnings.append(f"Clicked outside KWay news modal at {stage}")
                    await asyncio.sleep(0.3)
                if await news_modal.is_visible(timeout=300):
                    await page.keyboard.press("Escape")
                    self.warnings.append(f"Pressed Escape for KWay news modal at {stage}")
                    await asyncio.sleep(0.3)
        except Exception:
            pass

        # Generic selector-driven popup close.
        for selector in self.cfg.popup_close_selectors:
            if page.is_closed():
                page = await self.active_page(None)
            try:
                loc = page.locator(selector).first
                if await loc.count() and await loc.is_visible(timeout=500):
                    if selector == "#mpm_news_check" or "mpm_news_check" in selector:
                        await loc.check(timeout=1500, force=True)
                    else:
                        await loc.click(timeout=1500, force=True)
                    self.warnings.append(
                        f"Closed announcement/modal at {stage} via selector: {selector}"
                    )
                    await asyncio.sleep(0.3)
            except Exception:
                continue

        # Close stray announcement windows but never the last page.
        open_pages = [p for p in self.context.pages if not p.is_closed()]
        for popup in list(open_pages):
            if popup is page or popup.is_closed() or len(open_pages) <= 1:
                continue
            try:
                title = await popup.title()
                text = await popup.locator("body").inner_text(timeout=1000)
                blob = f"{title}\n{popup.url}\n{text[:500]}"
                if re.search(r"公告|通知|訊息|最新消息|news|notice|announce|bulletin", blob, re.I):
                    await popup.close()
                    open_pages = [p for p in self.context.pages if not p.is_closed()]
                    self.warnings.append(
                        f"Closed announcement popup window at {stage}: {title or popup.url}"
                    )
            except Exception:
                continue

        self.page = await self.active_page(page)
        return self.page

    async def has_link_text(self, candidates: list[str]) -> bool:
        """True if any of the link/button texts is visible on the current page."""
        if self.page.is_closed():
            return False
        for text in candidates:
            try:
                if await self.page.get_by_text(text, exact=False).count():
                    return True
            except Exception:
                pass
        try:
            return bool(
                await self.page.evaluate(
                    """
                    (needles) => Array.from(document.querySelectorAll('a,button,input,[onclick],td,span,div'))
                      .some((n) => {
                        const blob = n.innerText || n.value || n.title || n.getAttribute('onclick') || '';
                        return needles.some((needle) => blob.includes(needle));
                      })
                    """,
                    candidates,
                )
            )
        except Exception:
            return False

    async def open_portal_link(
        self,
        *,
        link_text_candidates: list[str],
        direct_url: str = "",
    ) -> Page:
        """Open a portal sub-page using the most reliable available method.

        Order: (1) fn_open()/window.open with a direct URL if provided —
        this preserves the portal's auth context the same way the real
        button does; (2) click the on-screen link text; (3) DOM-search
        fallback for old ASP.NET menus.
        """
        if direct_url:
            return await self._open_via_fn_open(direct_url)

        await self.close_announcement_popups("before portal link click")
        last_error: Exception | None = None
        for attempt in range(1, 5):
            try:
                await self.close_announcement_popups(f"portal link attempt {attempt}")
                new_page = await self._click_link_text(link_text_candidates)
                await self.safe_wait_networkidle(new_page)
                self.page = new_page
                await self.close_announcement_popups("after portal link click")
                return new_page
            except Exception as exc:
                last_error = exc
                self.warnings.append(
                    f"Portal link click attempt {attempt} failed: {type(exc).__name__}: {exc}"
                )
                try:
                    if attempt == 2:
                        # Re-load portal once in case the announcement flow
                        # closed the original page after login.
                        await self.page.goto(
                            self.cfg.portal_url,
                            wait_until="domcontentloaded",
                            timeout=self.cfg.timeout_ms,
                        )
                        await self.safe_wait_networkidle()
                    await self.page.wait_for_timeout(700)
                except Exception:
                    pass

        raise RuntimeError(
            "Failed to open portal link after popup handling. "
            "If the menu uses a generated URL, pass direct_url= explicitly. "
            f"Last error: {last_error}"
        )

    # ---- private: login flow ---------------------------------------------

    async def _install_context_handlers(self) -> None:
        async def on_dialog(dialog):
            try:
                self.warnings.append(
                    f"Accepted browser dialog: {dialog.type} {dialog.message[:80]}"
                )
                await dialog.accept()
            except Exception:
                pass

        async def on_new_page(popup: Page):
            try:
                popup.on("dialog", lambda d: asyncio.create_task(on_dialog(d)))
                await popup.wait_for_load_state("domcontentloaded", timeout=5000)
                title = await popup.title()
                text = await popup.locator("body").inner_text(timeout=1500)
                blob = f"{title}\n{popup.url}\n{text[:500]}"
                if re.search(r"公告|通知|訊息|最新消息|news|notice|announce|bulletin", blob, re.I):
                    self.warnings.append(
                        f"Detected announcement popup window: {title or popup.url}"
                    )
            except Exception:
                pass

        self.context.on("page", lambda p: asyncio.create_task(on_new_page(p)))
        for p in self.context.pages:
            p.on("dialog", lambda d: asyncio.create_task(on_dialog(d)))

    async def _login_if_needed(self, manual_login_timeout: int) -> None:
        await self.page.goto(
            self.cfg.portal_url,
            wait_until="domcontentloaded",
            timeout=self.cfg.timeout_ms,
        )
        await self.safe_wait_networkidle()
        await self.close_announcement_popups("after portal load")

        # Probe: if any common menu text is already visible, treat as logged in.
        common_menu_texts = ["預訂會議室", "預定會議室", "請假", "報帳"]
        if await self.has_link_text(common_menu_texts):
            return

        if not self.cfg.username or not self.cfg.password:
            raise RuntimeError("Missing KWAY_USERNAME / KWAY_PASSWORD in .env")

        username_candidates = [
            self.cfg.username_selector,
            'input[name="username"]',
            'input[name="user"]',
            'input[name="account"]',
            'input[name="UserName"]',
            'input[name="txtUser"]',
            'input[name="txtAccount"]',
            'input[id*="User" i]',
            'input[id*="Account" i]',
            'input[id*="Login" i][type="text"]',
            'input[type="email"]',
            'input[type="text"]',
        ]
        password_candidates = [
            self.cfg.password_selector,
            'input[name="password"]',
            'input[name="Password"]',
            'input[name="txtPassword"]',
            'input[id*="Pass" i]',
            'input[type="password"]',
        ]
        login_button_candidates = [
            self.cfg.login_button_selector,
            'button[type="submit"]',
            'input[type="submit"]',
            'input[value*="登入"]',
            'button:has-text("登入")',
            "text=登入",
            "text=Login",
        ]

        ok_user = await self._fill_first_visible(
            username_candidates, self.cfg.username, "username"
        )
        ok_pass = await self._fill_first_visible(
            password_candidates, self.cfg.password, "password"
        )
        if not ok_user or not ok_pass:
            self.warnings.append(
                "Login fields were not fully auto-detected. Fill KWAY_*_SELECTOR in .env."
            )

        if not await self._click_first_visible(login_button_candidates, "login button"):
            await self.page.keyboard.press("Enter")

        await self.safe_wait_networkidle()
        await self.close_announcement_popups("after login")

        if await self.has_link_text(common_menu_texts):
            return

        if manual_login_timeout > 0:
            await self._wait_for_manual_login(manual_login_timeout, common_menu_texts)
            return

        raise RuntimeError(
            "Login did not reach the portal home page. Run with --headed, "
            "finish login manually, or set precise login selectors in .env."
        )

    async def _fill_first_visible(
        self, selectors: list[str], value: str, label: str
    ) -> bool:
        for selector in selectors:
            if not selector:
                continue
            try:
                loc = self.page.locator(selector).first
                if await loc.count() and await loc.is_visible(timeout=1500):
                    await loc.fill(value)
                    return True
            except Exception:
                continue
        print(f"[warn] Cannot auto-fill {label}", file=sys.stderr)
        return False

    async def _click_first_visible(self, selectors: list[str], label: str) -> bool:
        for selector in selectors:
            if not selector:
                continue
            try:
                loc = self.page.locator(selector).first
                if await loc.count() and await loc.is_visible(timeout=1500):
                    await loc.click()
                    return True
            except Exception:
                continue
        print(f"[warn] Cannot auto-click {label}", file=sys.stderr)
        return False

    async def _wait_for_manual_login(
        self, timeout_sec: int, success_link_texts: list[str]
    ) -> None:
        deadline = time.monotonic() + timeout_sec
        print(
            f"[manual-login] Finish login in the opened browser within {timeout_sec}s...",
            file=sys.stderr,
        )
        while time.monotonic() < deadline:
            try:
                self.page = await self.active_page(self.page)
                await self.close_announcement_popups("manual login wait")
                if await self.has_link_text(success_link_texts):
                    self.warnings.append("Manual login detected; continuing.")
                    return
            except Exception as exc:
                self.warnings.append(
                    f"Manual login wait recovered: {type(exc).__name__}: {exc}"
                )
            await asyncio.sleep(1)
        raise RuntimeError("Manual login timed out before reaching portal home page.")

    # ---- private: link opening -------------------------------------------

    async def _open_via_fn_open(self, url: str) -> Page:
        """Use the portal's own `fn_open()` if defined, else window.open."""
        await self.close_announcement_popups("before fn_open")
        before_pages = set(self.context.pages)
        try:
            async with self.context.expect_page(timeout=8000) as popup_info:
                await self.page.evaluate(
                    """
                    (u) => {
                      if (typeof window.fn_open === 'function') {
                        window.fn_open(u);
                      } else {
                        window.open(u, '_blank');
                      }
                    }
                    """,
                    url,
                )
            popup = await popup_info.value
            await popup.wait_for_load_state("domcontentloaded", timeout=self.cfg.timeout_ms)
            await self.safe_wait_networkidle(popup)
            await self._wait_for_nonblank(popup, 10_000)
            self.warnings.append("Opened portal link via fn_open/window.open.")
            self.page = popup
            await self.close_announcement_popups("after fn_open")
            return popup
        except Exception as exc:
            self.warnings.append(
                f"fn_open path failed; falling back to direct goto: {type(exc).__name__}: {exc}"
            )
            for candidate in reversed(self.context.pages):
                if candidate not in before_pages and not candidate.is_closed():
                    self.page = candidate
                    return candidate
            new_page = await self.context.new_page()
            await new_page.goto(url, wait_until="domcontentloaded", timeout=self.cfg.timeout_ms)
            await self.safe_wait_networkidle(new_page)
            await self._wait_for_nonblank(new_page, 10_000)
            self.page = new_page
            await self.close_announcement_popups("after fallback goto")
            return new_page

    async def _click_link_text(self, candidates: list[str]) -> Page:
        before_pages = set(self.context.pages)

        # Try to extract a direct URL from the menu element first — works for
        # the KWay style `<button onclick="fn_open('...')">`.
        direct_url = await self.page.evaluate(
            r"""
            (needles) => {
              const nodes = Array.from(document.querySelectorAll('a,button,input,[onclick],td,span,div'));
              const el = nodes.find((n) => {
                const blob = n.innerText || n.value || n.title || n.getAttribute('onclick') || '';
                return needles.some((needle) => blob.includes(needle));
              });
              if (!el) return null;
              const href = el.getAttribute('href');
              if (href && href !== '#') return href;
              const onclick = el.getAttribute('onclick') || '';
              const m = onclick.match(/fn_open\(['"]([^'"]+)['"]\)/i) ||
                        onclick.match(/window\.open\(['"]([^'"]+)['"]\)/i);
              return m ? m[1] : null;
            }
            """,
            candidates,
        )
        if direct_url:
            return await self._open_via_fn_open(direct_url)

        clicked = False
        for text in candidates:
            try:
                await self.page.get_by_text(text, exact=False).first.click(
                    timeout=self.cfg.timeout_ms
                )
                clicked = True
                break
            except Exception:
                try:
                    await self.page.get_by_role(
                        "link", name=re.compile(re.escape(text))
                    ).first.click(timeout=self.cfg.timeout_ms)
                    clicked = True
                    break
                except Exception:
                    continue

        if not clicked:
            clicked = await self.page.evaluate(
                """
                (needles) => {
                  const nodes = Array.from(document.querySelectorAll('a,button,input,[onclick],td,span,div'));
                  const el = nodes.find((n) => {
                    const blob = n.innerText || n.value || n.title || n.getAttribute('onclick') || '';
                    return needles.some((needle) => blob.includes(needle));
                  });
                  if (!el) return false;
                  el.click();
                  return true;
                }
                """,
                candidates,
            )
            if not clicked:
                raise RuntimeError(f"Cannot find link text: {', '.join(candidates)}")

        await asyncio.sleep(1)
        for candidate in reversed(self.context.pages):
            if candidate not in before_pages and not candidate.is_closed():
                return candidate
        return await self.active_page(self.page)

    async def _wait_for_nonblank(self, page: Page, timeout_ms: int) -> None:
        deadline = time.monotonic() + (timeout_ms / 1000)
        while time.monotonic() < deadline:
            try:
                text = (await page.locator("body").inner_text(timeout=1000)).strip()
                html = (await page.locator("body").inner_html(timeout=1000)).strip()
                if text or len(html) > 50:
                    return
            except Exception:
                pass
            await asyncio.sleep(0.5)


# Re-export for type-checkers
__all__ = ["PortalSession"]
