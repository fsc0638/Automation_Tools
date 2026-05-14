"""Playwright lifecycle helpers (launch / context / cleanup).

PortalSession orchestrates login + navigation; this module only worries
about how the browser process is started and torn down.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from playwright.async_api import (
    Browser,
    BrowserContext,
    Page,
    Playwright,
    async_playwright,
)


@dataclass
class BrowserHandle:
    """Owns the Playwright objects so callers can close them in the right order.

    `browser` is None when launched in persistent-context mode (used for
    `--keep-signed-in` to reuse cookies/SSO).

    `close()` MUST be called to release the underlying Node subprocess —
    without `playwright.stop()` Python's asyncio reports `unclosed transport`
    warnings on shutdown.
    """

    playwright: Playwright
    context: BrowserContext
    page: Page
    browser: Browser | None

    async def close(self) -> None:
        try:
            await self.context.close()
        finally:
            try:
                if self.browser is not None:
                    await self.browser.close()
            finally:
                await self.playwright.stop()


async def launch_browser(
    *,
    headed: bool,
    slow_mo: int,
    keep_signed_in: bool,
    profile_dir: Path,
) -> BrowserHandle:
    """Start Chromium with the right context flavour.

    Returns a BrowserHandle. Caller is responsible for calling .close().
    """
    p = await async_playwright().start()
    launch_kwargs = {"headless": not headed, "slow_mo": slow_mo}

    if keep_signed_in:
        # Persistent context reuses cookies/local storage across runs.
        # Useful for portals with SSO or MFA where re-login is annoying.
        profile_dir.mkdir(parents=True, exist_ok=True)
        context = await p.chromium.launch_persistent_context(
            str(profile_dir), **launch_kwargs
        )
        page = context.pages[0] if context.pages else await context.new_page()
        return BrowserHandle(playwright=p, context=context, page=page, browser=None)

    browser = await p.chromium.launch(**launch_kwargs)
    context = await browser.new_context(locale="zh-TW", timezone_id="Asia/Taipei")
    page = await context.new_page()
    return BrowserHandle(playwright=p, context=context, page=page, browser=browser)
