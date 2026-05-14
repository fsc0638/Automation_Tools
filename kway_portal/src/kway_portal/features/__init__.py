"""Feature registry.

Adding a new feature: create a module that subclasses PortalFeature and
imports it here. The registry dict is what cli.py reads to build subparsers.
"""

from __future__ import annotations

from .base import PortalFeature
from .meeting_rooms import MeetingRoomsFeature

REGISTRY: dict[str, type[PortalFeature]] = {
    MeetingRoomsFeature.name: MeetingRoomsFeature,
    # Future:
    # LeaveFeature.name: LeaveFeature,
    # ExpenseFeature.name: ExpenseFeature,
}

__all__ = ["PortalFeature", "REGISTRY"]
