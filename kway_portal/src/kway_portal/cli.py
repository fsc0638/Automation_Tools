"""Command-line entry point.

Invoke via:
  python -m kway_portal <feature> [...]
or, after `pip install -e .`:
  kway-portal <feature> [...]

Each feature module registers itself in `features.REGISTRY`; this file just
builds the argparse tree from that registry and dispatches.
"""

from __future__ import annotations

import argparse
import asyncio
import sys
from pathlib import Path

from .config import default_output_dir, default_snapshot_dir, load_config
from .features import REGISTRY
from .output import feature_dir, timestamp, write_json
from .scheduling import seconds_until_hhmm
from .session import PortalSession


def _add_common_args(sp: argparse.ArgumentParser) -> None:
    """Args shared by every feature: browser + scheduling controls."""
    sp.add_argument("--headed", action="store_true", help="Show the browser window")
    sp.add_argument("--slow-mo", type=int, default=0, help="Playwright slow_mo ms")
    sp.add_argument(
        "--keep-signed-in",
        action="store_true",
        help="Reuse persistent browser profile under .browser-profile/",
    )
    sp.add_argument(
        "--manual-login-timeout",
        type=int,
        default=0,
        help="Seconds to wait for manual login if auto-login fails (default 300 in --headed mode).",
    )
    sp.add_argument(
        "--once",
        action="store_true",
        help="Run once and exit (default behaviour when no schedule is set)",
    )
    sp.add_argument(
        "--interval-minutes",
        type=int,
        default=0,
        help="Run repeatedly every N minutes",
    )
    sp.add_argument(
        "--schedule-at",
        default="",
        help="Run daily at HH:MM, e.g. 08:30",
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="kway-portal",
        description="KWay corporate portal automation client.",
    )
    sub = parser.add_subparsers(dest="feature", required=True)

    for name, feat_cls in REGISTRY.items():
        sp = sub.add_parser(name, help=feat_cls.description, description=feat_cls.description)
        _add_common_args(sp)
        feat_cls.add_arguments(sp)
    return parser


async def _run_feature_once(args: argparse.Namespace) -> Path:
    cfg = load_config()
    feat_cls = REGISTRY[args.feature]
    feature = feat_cls()

    out_root = default_output_dir()
    snap_root = default_snapshot_dir()
    out_dir = feature_dir(out_root, feat_cls.name)
    snap_dir = feature_dir(snap_root, feat_cls.name)

    manual_timeout = args.manual_login_timeout or (300 if args.headed else 0)
    session = await PortalSession.open(
        cfg,
        headed=args.headed,
        slow_mo=args.slow_mo,
        keep_signed_in=args.keep_signed_in,
        manual_login_timeout=manual_timeout,
        profile_dir=Path.cwd() / ".browser-profile",
    )
    try:
        result = await feature.run(
            session, args, output_dir=out_dir, snapshot_dir=snap_dir
        )
    finally:
        await session.close()

    out_path = Path(args.output) if getattr(args, "output", "") else out_dir / _default_filename(args)
    write_json(out_path, result)
    print(f"[ok] wrote {out_path}")
    if session.warnings:
        print("[warn] " + " | ".join(session.warnings), file=sys.stderr)
    return out_path


def _default_filename(args: argparse.Namespace) -> str:
    """Per-feature default file name. Falls back to <feature>_<timestamp>.json."""
    ts = timestamp()
    start = getattr(args, "start", "")
    end = getattr(args, "end", "")
    if start and end:
        return f"{args.feature}_{start}_{end}_{ts}.json"
    return f"{args.feature}_{ts}.json"


async def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    one_shot = args.once or (not args.interval_minutes and not args.schedule_at)
    if one_shot:
        await _run_feature_once(args)
        return 0

    while True:
        if args.schedule_at:
            wait = seconds_until_hhmm(args.schedule_at)
            print(f"[schedule] next run at {args.schedule_at}; sleeping {wait}s")
            await asyncio.sleep(wait)
        await _run_feature_once(args)
        if args.interval_minutes:
            wait = args.interval_minutes * 60
            print(f"[schedule] sleeping {wait}s")
            await asyncio.sleep(wait)


def main_sync() -> int:
    """Entry point for the `kway-portal` console script."""
    return asyncio.run(main())
