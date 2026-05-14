"""Feature abstract base class.

Every feature handles one logical portal task (e.g. scrape meeting-room
bookings, submit a leave request, fetch announcements). Features share the
authenticated PortalSession; they only own the work that comes AFTER login.
"""

from __future__ import annotations

import argparse
from abc import ABC, abstractmethod
from pathlib import Path
from typing import Any

from ..session import PortalSession


class PortalFeature(ABC):
    """Subclass this for each portal operation we want to automate.

    Subclasses set `name` (the CLI subcommand, e.g. "meeting-rooms") and
    `description` (a one-line summary for `--help`). Implement
    `add_arguments` to attach feature-specific CLI flags and `run` to do
    the work.

    The runner passes already-resolved output/snapshot directories so each
    feature does not have to re-implement the namespacing convention.
    """

    name: str = ""
    description: str = ""

    @classmethod
    @abstractmethod
    def add_arguments(cls, subparser: argparse.ArgumentParser) -> None:
        """Add feature-specific arguments to the CLI subparser."""

    @abstractmethod
    async def run(
        self,
        session: PortalSession,
        args: argparse.Namespace,
        *,
        output_dir: Path,
        snapshot_dir: Path,
    ) -> dict[str, Any]:
        """Execute the feature. Return a JSON-serialisable result dict.

        The CLI is responsible for writing the result to disk; features
        should return data, not write files themselves (snapshots are an
        exception — they are saved via output.save_snapshot()).
        """
