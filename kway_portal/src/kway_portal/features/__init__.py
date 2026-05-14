"""Feature registry.

Adding a new feature: create a module that subclasses PortalFeature and
imports it here. The registry dict is what cli.py reads to build subparsers.
"""

from __future__ import annotations

from .base import PortalFeature
from .employee_directory import EmployeeDirectoryFeature
from .meeting_book import MeetingBookFeature
from .meeting_rooms import MeetingRoomsFeature

REGISTRY: dict[str, type[PortalFeature]] = {
    MeetingRoomsFeature.name: MeetingRoomsFeature,
    EmployeeDirectoryFeature.name: EmployeeDirectoryFeature,
    MeetingBookFeature.name: MeetingBookFeature,
}

__all__ = ["PortalFeature", "REGISTRY"]
