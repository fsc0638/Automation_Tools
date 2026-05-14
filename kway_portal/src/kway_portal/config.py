"""Shared configuration for the KWay portal client.

Convention: every env var starts with `KWAY_`. Variables that apply to the
whole portal session use `KWAY_<NAME>`. Variables scoped to a feature use
`KWAY_<FEATURE>_<NAME>` (e.g. `KWAY_MEETING_ROOMS_BOOKING_URL`). Feature
modules read their own namespace; this file only holds the session-wide
configuration that PortalSession needs to log in and dismiss popups.
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path

try:
    from dotenv import load_dotenv
except ImportError:
    load_dotenv = None  # type: ignore[assignment]


PORTAL_URL = "https://portal.kway.com.tw/Index.aspx"

# Default popup-close selectors. Kept in code (not just .env) so a fresh
# clone has reasonable defaults; users add to this via KWAY_POPUP_CLOSE_SELECTORS.
DEFAULT_POPUP_CLOSE_SELECTORS: tuple[str, ...] = (
    "#mpm_news_check",
    'input#mpm_news_check',
    'input[type="checkbox"][onclick*="index_close_news"]',
    'button:has-text("關閉")',
    'button:has-text("關 關")',
    'button:has-text("確定")',
    'button:has-text("知道了")',
    'input[value*="關閉"]',
    'input[value*="確定"]',
    'a:has-text("關閉")',
    ".modal .close",
    '.modal [data-dismiss="modal"]',
    ".ui-dialog-titlebar-close",
    ".swal2-confirm",
    '[aria-label="Close"]',
    '[aria-label="close"]',
    ".close",
)


@dataclass
class PortalConfig:
    """Session-wide config shared by every feature.

    Per-feature settings (e.g. KWAY_MEETING_ROOMS_BOOKING_URL) are read inside
    the feature module — they do not belong here.
    """

    username: str
    password: str
    portal_url: str = PORTAL_URL
    timeout_ms: int = 30_000
    popup_close_selectors: list[str] = field(default_factory=list)

    # Optional explicit login-form selectors. Kept here because login is a
    # shared concern, not feature-specific.
    username_selector: str = ""
    password_selector: str = ""
    login_button_selector: str = ""


def _env(name: str, default: str = "") -> str:
    return os.environ.get(name, default).strip()


def load_config(*, dotenv_path: Path | None = None) -> PortalConfig:
    """Load PortalConfig from environment / .env.

    Pass `dotenv_path` to override the default search location (which is the
    package root's ../.env, i.e. <project>/kway_portal/.env).
    """
    if load_dotenv is not None:
        target = dotenv_path or default_env_path()
        if target and target.exists():
            load_dotenv(target)

    extra_selectors = [
        s.strip()
        for s in _env("KWAY_POPUP_CLOSE_SELECTORS").split(",")
        if s.strip()
    ]
    selectors = list(DEFAULT_POPUP_CLOSE_SELECTORS) + extra_selectors

    return PortalConfig(
        username=_env("KWAY_USERNAME"),
        password=_env("KWAY_PASSWORD"),
        portal_url=_env("KWAY_PORTAL_URL", PORTAL_URL),
        timeout_ms=int(_env("KWAY_TIMEOUT_MS", "30000")),
        popup_close_selectors=selectors,
        username_selector=_env("KWAY_USERNAME_SELECTOR"),
        password_selector=_env("KWAY_PASSWORD_SELECTOR"),
        login_button_selector=_env("KWAY_LOGIN_BUTTON_SELECTOR"),
    )


def default_env_path() -> Path:
    """Path to <project_root>/kway_portal/.env when running from the package."""
    # src/kway_portal/config.py → src/kway_portal → src → <package_root>
    return Path(__file__).resolve().parents[2] / ".env"


def default_output_dir() -> Path:
    return Path(__file__).resolve().parents[2] / "output"


def default_snapshot_dir() -> Path:
    return Path(__file__).resolve().parents[2] / "snapshots"
