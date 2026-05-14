# KWay Portal Bridge

Python + Playwright client for `portal.kway.com.tw`. One process owns the
authenticated browser session; every portal-side automation (meeting-room
scraping, leave applications, expense reports, …) is implemented as a
*feature* under `src/kway_portal/features/`.

## Install

```powershell
cd kway_portal
python -m venv .venv
.venv\Scripts\Activate.ps1   # PowerShell; use `.venv/bin/activate` on macOS/Linux
pip install -e .
python -m playwright install chromium
```

Copy `.env.example` to `.env` and fill in `KWAY_USERNAME` / `KWAY_PASSWORD`.

## Usage

```powershell
# Show all features
python -m kway_portal --help

# Scrape meeting-room bookings for a range (per-day loop)
python -m kway_portal meeting-rooms --start 2026-05-14 --end 2026-05-31

# Debug with a visible browser; pauses up to 300s for manual login
python -m kway_portal meeting-rooms --headed --start 2026-05-14 --end 2026-05-14

# Reuse persistent profile (helpful when SSO/MFA is in the way)
python -m kway_portal meeting-rooms --headed --keep-signed-in --start 2026-05-14 --end 2026-05-14

# Scheduled runs (every 60 min / once daily at 08:30)
python -m kway_portal meeting-rooms --interval-minutes 60 --start 2026-05-14 --end 2026-05-31
python -m kway_portal meeting-rooms --schedule-at 08:30 --start 2026-05-14 --end 2026-05-31
```

After `pip install -e .` the same commands work as `kway-portal meeting-rooms ...`.

### Output layout

```
output/
└── meeting-rooms/
    └── meeting-rooms_2026-05-14_2026-05-31_<timestamp>.json

snapshots/
└── meeting-rooms/
    ├── meeting_rooms_2026-05-14_<timestamp>.html
    └── meeting_rooms_2026-05-14_<timestamp>.png
```

Both directories are gitignored. The snapshots are useful when a selector
breaks because the portal changed shape — you can re-inspect the exact page
the scraper saw.

## Architecture

```
src/kway_portal/
├── cli.py            ← argparse, subcommand dispatch
├── config.py         ← PortalConfig + .env loader
├── browser.py        ← Playwright launch / persistent context
├── session.py        ← PortalSession: login, popup, navigation helpers
├── output.py         ← JSON + snapshot writers (per-feature dirs)
├── scheduling.py     ← --interval-minutes / --schedule-at helpers
└── features/
    ├── base.py       ← PortalFeature ABC
    └── meeting_rooms.py
```

### Adding a new feature

1. Create `src/kway_portal/features/<feature_name>.py` with a class that
   subclasses `PortalFeature`:

   ```python
   class LeaveFeature(PortalFeature):
       name = "leave"
       description = "List or submit leave applications."

       @classmethod
       def add_arguments(cls, sp):
           sp.add_argument("--status", default="pending")

       async def run(self, session, args, *, output_dir, snapshot_dir):
           page = await session.open_portal_link(
               link_text_candidates=["請假申請"],
               direct_url=os.getenv("KWAY_LEAVE_URL", ""),
           )
           ...
           return {"records": [...]}
   ```

2. Register it in `features/__init__.py`:

   ```python
   from .leave import LeaveFeature
   REGISTRY = {
       MeetingRoomsFeature.name: MeetingRoomsFeature,
       LeaveFeature.name: LeaveFeature,
   }
   ```

3. Add any `KWAY_<FEATURE>_<NAME>` env vars to `.env.example`.

The CLI picks up the new subcommand automatically. The feature inherits
all shared flags (`--headed`, `--keep-signed-in`, `--once`,
`--interval-minutes`, `--schedule-at`, ...).

## Integration with the main backend

This package writes JSON files. A future `import_portal_*` binary in
`backend/src/bin/` will read those files and upsert into the relevant
tables (e.g. `meetings`). That importer is a separate change so its
behaviour can be designed against real scraped data.

## Notes

- KWay's portal mixes Big5 content with utf-8 meta tags; do not screen-scrape
  the raw HTML, use `page.evaluate()` to let the browser handle decoding.
- The booking-room page is a **single-day** view; the `--start/--end` range
  is iterated day-by-day via the page's `setdate` input.
- See [parent README](../README.md#kway-portal-bridge) for how this fits
  into the larger Kway In-house Dev Automation Tools project.
