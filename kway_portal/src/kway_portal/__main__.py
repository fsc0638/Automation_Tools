"""Enables `python -m kway_portal …`."""

from .cli import main_sync

if __name__ == "__main__":
    raise SystemExit(main_sync())
