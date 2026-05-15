"""Render the AgentK meeting flow & architecture as Python-driven diagrams.

This script captures the AgentK (kway-rdc/AgentK @ agent/leader/workflow,
HEAD 3dd5181) meeting subsystem — domain model, lifecycle, permissions,
APIs, and the three main user flows — as Python data structures and emits:

  - er_diagram.png            (entities + relations, Graphviz)
  - lifecycle.png             (status / lock state machine, Graphviz)
  - create_to_close.mmd       (Mermaid sequence: create → schedule → end)
  - record_generation.mmd     (Mermaid sequence: AI minutes + task sync)
  - permission_matrix.md      (role × action grid)
  - api_surface.md            (endpoint table)
  - role_mapping.md           (AgentK ↔ Kway Dev rename map)
  - README.md                 (index)

All role names in the output use **Kway Dev terminology** per the mapping:

    AgentK admin    →  Kway Dev owner   (workspace-wide highest)
    AgentK owner    →  Kway Dev admin   (per-meeting primary)
    AgentK delegate →  Kway Dev editor  (per-meeting deputy)
    AgentK attendee →  Kway Dev viewer  (per-meeting member)

Memory-weight policy in Kway Dev is unchanged: this file documents what
AgentK *does*, expressed in Kway Dev's vocabulary so our team can read it
without translating in their head.

Requires: graphviz (pip install graphviz) plus the `dot` binary on PATH.
Mermaid files are emitted as .mmd text — paste into https://mermaid.live
or run `npx -p @mermaid-js/mermaid-cli mmdc -i x.mmd -o x.png` to render.

Usage:
    python render_meeting_flow.py [--out OUT_DIR]
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass, field
from pathlib import Path
from typing import Literal


# ─────────────────────────────────────────────────────────────────────────
# Role mapping  —  Kway Dev names are canonical in this file
# ─────────────────────────────────────────────────────────────────────────

ROLE_MAP: dict[str, str] = {
    # AgentK term → Kway Dev term (Kway Dev is what we display everywhere)
    "admin": "owner",
    "owner": "admin",
    "delegate": "editor",
    "attendee": "viewer",
}

# Highest authority first. Used to print the permission ladder.
KWAY_ROLE_LADDER: list[str] = ["owner", "admin", "editor", "viewer"]


# ─────────────────────────────────────────────────────────────────────────
# Domain model  —  3 meeting tables + 1 shared assets table
# ─────────────────────────────────────────────────────────────────────────

@dataclass(frozen=True)
class Column:
    name: str
    sql_type: str
    note: str = ""


@dataclass(frozen=True)
class Table:
    name: str
    columns: list[Column]
    notes: str = ""


MEETINGS = Table(
    name="meetings",
    columns=[
        Column("id", "uuid", "PK"),
        Column("workspace_id", "uuid", "FK workspaces"),
        Column("room_id", "uuid", "FK rooms, UNIQUE (1:1 dedicated room)"),
        Column("title", "varchar(240)"),
        Column("description", "text"),
        Column("status", "varchar(32)", "draft | scheduled | ended | cancelled"),
        Column("is_locked", "bool", "independent lock flag (ended defaults to true)"),
        Column("starts_at", "timestamptz"),
        Column("ends_at", "timestamptz"),
        Column("location", "varchar(240)"),
        Column("join_url", "varchar(1000)"),
        Column("external_provider", "varchar(80)", "reserved, no sync"),
        Column("external_event_id", "varchar(255)", "reserved"),
        Column("external_event_url", "varchar(1000)", "reserved"),
        Column("sync_status", "varchar(40)", "reserved"),
        Column("last_synced_at", "timestamptz", "reserved"),
        Column("created_by_user_id", "uuid", "FK users"),
        Column("updated_by_user_id", "uuid", "FK users"),
        Column("created_at", "timestamptz"),
        Column("updated_at", "timestamptz"),
    ],
    notes="Indexes: workspace_id, room_id, is_locked, (workspace_id,status), (workspace_id,starts_at)",
)

MEETING_PARTICIPANTS = Table(
    name="meeting_participants",
    columns=[
        Column("id", "uuid", "PK"),
        Column("meeting_id", "uuid", "FK meetings"),
        Column("user_id", "uuid", "FK users"),
        Column("role", "varchar(32)", "admin | editor | viewer  (Kway Dev terms)"),
        Column("created_at", "timestamptz"),
        Column("updated_at", "timestamptz"),
    ],
    notes="UNIQUE(meeting_id, user_id). Workspace `owner` (AgentK 'admin') is not "
          "stored here — it comes from workspace_members.",
)

ASSETS = Table(
    name="assets",
    columns=[
        Column("id", "uuid", "PK"),
        Column("workspace_id", "uuid"),
        Column("room_id", "uuid", "nullable; meeting files set this"),
        Column("uploaded_by_user_id", "uuid"),
        Column("original_filename", "varchar(255)"),
        Column("content_type", "varchar(120)"),
        Column("asset_kind", "varchar(40)", "meeting_file | audio | ..."),
        Column("size_bytes", "int"),
        Column("storage_provider", "varchar(40)"),
        Column("storage_key", "varchar(1024)", "UNIQUE"),
        Column("status", "varchar(40)", "active | soft_deleted | hard_deleted"),
        Column("metadata_json", "jsonb"),
        Column("deleted_at", "timestamptz", "soft-delete stamp"),
        Column("soft_deleted_until", "timestamptz", "+30 days; user-visible grace"),
        Column("hard_delete_after", "timestamptz", "+60 days; sweep worker target"),
        Column("created_at", "timestamptz"),
        Column("updated_at", "timestamptz"),
    ],
    notes="Generic asset layer (not meeting-only). audio_transcripts.asset_id FK joins here.",
)

MEETING_RECORDS = Table(
    name="meeting_records",
    columns=[
        Column("id", "uuid", "PK"),
        Column("workspace_id", "uuid"),
        Column("meeting_id", "uuid", "FK meetings, UNIQUE (1:1)"),
        Column("room_id", "uuid"),
        Column("summary", "text"),
        Column("decisions_json", "jsonb", "string[]"),
        Column("action_items_json", "jsonb",
               "{title, description, assignee_user_id, source}[]"),
        Column("transcript_ids_json", "jsonb", "refs to audio_transcripts"),
        Column("ai_job_ids_json", "jsonb", "refs to ai_jobs that produced this"),
        Column("task_ids_json", "jsonb", "refs to synced tasks"),
        Column("updated_by_user_id", "uuid"),
        Column("created_at", "timestamptz"),
        Column("updated_at", "timestamptz"),
    ],
    notes="Record stores only IDs of transcripts / ai_jobs / tasks. Service does the JOIN.",
)

TABLES = [MEETINGS, MEETING_PARTICIPANTS, MEETING_RECORDS, ASSETS]


# ─────────────────────────────────────────────────────────────────────────
# Lifecycle  —  status × lock state machine
# ─────────────────────────────────────────────────────────────────────────

@dataclass(frozen=True)
class StateTransition:
    src: str
    dst: str
    trigger: str
    side_effect: str = ""


LIFECYCLE_TRANSITIONS: list[StateTransition] = [
    StateTransition("⊕ start", "draft", "POST /meetings", "default; room created same txn"),
    StateTransition("draft", "scheduled", "PATCH status=scheduled",
                    "requires starts_at + ends_at; emit meeting.scheduled"),
    StateTransition("draft", "cancelled", "PATCH status=cancelled", ""),
    StateTransition("scheduled", "ended", "PATCH status=ended",
                    "auto is_locked=true; room writes blocked"),
    StateTransition("scheduled", "cancelled", "PATCH status=cancelled", ""),
    StateTransition("ended", "scheduled", "PATCH is_locked=false (owner/admin)",
                    "reopen; status stays 'ended', lock cleared"),
    StateTransition("ended", "⊖ purged", "retention worker (stub)", "30+30 days"),
    StateTransition("cancelled", "⊖ purged", "retention worker (stub)", "30+30 days"),
]


# ─────────────────────────────────────────────────────────────────────────
# Permission matrix  —  Kway Dev role × action
# ─────────────────────────────────────────────────────────────────────────

# Kway Dev roles in descending authority (workspace owner highest, viewer lowest).
# workspace.owner is workspace-scoped; the others come from meeting_participants.
PERMISSION_ROLES = ["workspace.owner", "admin", "editor", "viewer", "non_participant"]

# Y = allowed, N = blocked, L = blocked when meeting is locked
PERMISSION_MATRIX: dict[str, dict[str, str]] = {
    "View meeting details (full)":      {"workspace.owner": "Y", "admin": "Y", "editor": "Y", "viewer": "Y", "non_participant": "N (busy only)"},
    "Edit meeting metadata":            {"workspace.owner": "Y", "admin": "Y", "editor": "Y", "viewer": "N", "non_participant": "N"},
    "Reassign admin / change owner":    {"workspace.owner": "Y", "admin": "Y", "editor": "N", "viewer": "N", "non_participant": "N"},
    "Add / remove participants":        {"workspace.owner": "Y", "admin": "Y", "editor": "N", "viewer": "N", "non_participant": "N"},
    "End / cancel meeting":             {"workspace.owner": "Y", "admin": "Y", "editor": "Y", "viewer": "N", "non_participant": "N"},
    "Reopen (clear is_locked)":         {"workspace.owner": "Y", "admin": "Y", "editor": "N", "viewer": "N", "non_participant": "N"},
    "Upload file":                      {"workspace.owner": "Y", "admin": "Y", "editor": "Y", "viewer": "N", "non_participant": "N", },
    "Download / preview file":          {"workspace.owner": "Y", "admin": "Y", "editor": "Y", "viewer": "Y", "non_participant": "N"},
    "Delete file (soft)":               {"workspace.owner": "Y", "admin": "Y", "editor": "Y", "viewer": "N", "non_participant": "N"},
    "Add transcript entry":             {"workspace.owner": "Y", "admin": "Y", "editor": "Y", "viewer": "N", "non_participant": "N"},
    "Edit record (summary/decisions)":  {"workspace.owner": "Y", "admin": "Y", "editor": "Y", "viewer": "N", "non_participant": "N"},
    "Generate AI minutes":              {"workspace.owner": "Y", "admin": "Y", "editor": "Y", "viewer": "N", "non_participant": "N"},
    "Sync action items → tasks":        {"workspace.owner": "Y", "admin": "Y", "editor": "Y", "viewer": "N", "non_participant": "N"},
    "Send messages in meeting room":    {"workspace.owner": "L", "admin": "L", "editor": "L", "viewer": "L", "non_participant": "N"},
}


# ─────────────────────────────────────────────────────────────────────────
# API surface
# ─────────────────────────────────────────────────────────────────────────

@dataclass(frozen=True)
class Endpoint:
    method: str
    path: str
    auth: str  # which Kway Dev role is the minimum bar
    purpose: str


ENDPOINTS: list[Endpoint] = [
    Endpoint("GET",    "/workspaces/{wsid}/meetings",                                   "any ws member",  "List meetings (busy-masked for non-participants)"),
    Endpoint("GET",    "/workspaces/{wsid}/meetings/calendar",                          "any ws member",  "Calendar view; entries flagged visibility=full|busy"),
    Endpoint("POST",   "/workspaces/{wsid}/meetings",                                   "any ws member",  "Create meeting + dedicated room in one txn"),
    Endpoint("GET",    "/workspaces/{wsid}/meetings/{mid}",                             "viewer+",        "Full detail"),
    Endpoint("PATCH",  "/workspaces/{wsid}/meetings/{mid}",                             "editor+",        "Update metadata / status / lock"),
    Endpoint("GET",    "/workspaces/{wsid}/meetings/{mid}/record",                      "viewer+",        "Read record aggregate"),
    Endpoint("PATCH",  "/workspaces/{wsid}/meetings/{mid}/record",                      "editor+",        "Hand-edit summary/decisions/action_items"),
    Endpoint("POST",   "/workspaces/{wsid}/meetings/{mid}/records/transcripts",         "editor+",        "Attach transcript entry"),
    Endpoint("POST",   "/workspaces/{wsid}/meetings/{mid}/record/generate",             "editor+",        "Trigger AI minutes via ai_jobs"),
    Endpoint("POST",   "/workspaces/{wsid}/meetings/{mid}/record/tasks/sync",           "editor+",        "Sync action_items into room tasks (dedup by title)"),
    Endpoint("GET",    "/workspaces/{wsid}/meetings/{mid}/files",                       "viewer+",        "List files"),
    Endpoint("POST",   "/workspaces/{wsid}/meetings/{mid}/files",                       "editor+",        "Upload file (writes assets row)"),
    Endpoint("GET",    "/workspaces/{wsid}/meetings/{mid}/files/{fid}/download",        "viewer+",        "Download"),
    Endpoint("DELETE", "/workspaces/{wsid}/meetings/{mid}/files/{fid}",                 "editor+",        "Soft-delete (60-day retention)"),
]

# Notable absence: there is NO `DELETE /meetings/{id}`. AgentK uses
# `PATCH status=cancelled` + retention worker instead.


# ─────────────────────────────────────────────────────────────────────────
# Realtime events
# ─────────────────────────────────────────────────────────────────────────

@dataclass(frozen=True)
class RealtimeEvent:
    name: str
    fired_by: str
    audience: str
    payload: str


REALTIME_EVENTS: list[RealtimeEvent] = [
    RealtimeEvent("meeting.created",              "POST",                    "participants + ws.owner", "full meeting payload"),
    RealtimeEvent("meeting.updated",              "PATCH metadata",          "participants + ws.owner", "diff fields"),
    RealtimeEvent("meeting.scheduled",            "PATCH status=scheduled",  "participants + ws.owner; busy-masked broadcast to other ws members", "full to participants, masked to others"),
    RealtimeEvent("meeting.ended",                "PATCH status=ended",      "participants + ws.owner", "meeting id + ended_at"),
    RealtimeEvent("meeting.cancelled",            "PATCH status=cancelled",  "participants + ws.owner", "meeting id + cancelled_at"),
    RealtimeEvent("meeting.participants_changed", "PATCH editor/viewer set", "participants + ws.owner", "before/after participant ids"),
    RealtimeEvent("meeting.record_updated",       "any record mutation",     "participants",            "record diff"),
    RealtimeEvent("meeting.lock_changed",         "PATCH is_locked",         "participants + ws.owner", "{is_locked}"),
]


# ─────────────────────────────────────────────────────────────────────────
# Renderers
# ─────────────────────────────────────────────────────────────────────────

def render_er_diagram(out_dir: Path) -> Path:
    """Entity-relationship diagram via Graphviz."""
    from graphviz import Digraph  # type: ignore[import]

    g = Digraph("agentk_meeting_er", format="png")
    g.attr(rankdir="LR", fontname="Helvetica", fontsize="11",
           label="AgentK Meeting ER  (Kway Dev role names)", labelloc="t")
    g.attr("node", shape="record", fontname="Helvetica", fontsize="10")

    def node(t: Table) -> None:
        rows = "\\l".join(f"{c.name} : {c.sql_type}" for c in t.columns)
        g.node(t.name, label=f"{{ <hdr> {t.name} | {rows}\\l }}")

    # External tables we reference but don't define here
    for ext in ("workspaces", "rooms", "users", "workspace_members",
                "audio_transcripts", "ai_jobs", "tasks"):
        g.node(ext, label=ext, shape="oval", style="dashed",
               color="#94a3b8", fontcolor="#475569")

    for t in TABLES:
        node(t)

    edges: list[tuple[str, str, str]] = [
        ("meetings", "workspaces", "workspace_id"),
        ("meetings", "rooms", "room_id (1:1)"),
        ("meetings", "users", "created_by"),
        ("meeting_participants", "meetings", "meeting_id"),
        ("meeting_participants", "users", "user_id"),
        ("meeting_records", "meetings", "meeting_id (1:1)"),
        ("meeting_records", "rooms", "room_id"),
        ("assets", "workspaces", "workspace_id"),
        ("assets", "rooms", "room_id"),
        ("assets", "users", "uploaded_by"),
        ("audio_transcripts", "assets", "asset_id"),
        ("workspace_members", "workspaces", "workspace_id"),
    ]
    for s, d, label in edges:
        g.edge(s, d, label=label, fontsize="8", color="#64748b")

    path = out_dir / "er_diagram"
    g.render(str(path), cleanup=True)
    return path.with_suffix(".png")


def render_lifecycle(out_dir: Path) -> Path:
    """Status × lock state machine via Graphviz."""
    from graphviz import Digraph  # type: ignore[import]

    g = Digraph("agentk_meeting_lifecycle", format="png")
    g.attr(rankdir="LR", fontname="Helvetica", fontsize="11",
           label="AgentK Meeting Lifecycle  —  status × is_locked",
           labelloc="t")
    g.attr("node", shape="box", style="rounded,filled", fontname="Helvetica",
           fontsize="10", fillcolor="#eff6ff", color="#0050a0")

    # Special states
    g.node("⊕ start", shape="circle", style="filled", fillcolor="#1a1a2e",
           fontcolor="white")
    g.node("⊖ purged", shape="doublecircle", style="filled",
           fillcolor="#64748b", fontcolor="white")
    # Highlight locked-default state
    g.node("ended", fillcolor="#fef3c7", color="#b45309",
           label="ended\\n(is_locked=true)")
    g.node("cancelled", fillcolor="#fee2e2", color="#991b1b")

    for tr in LIFECYCLE_TRANSITIONS:
        label = tr.trigger
        if tr.side_effect:
            label += f"\\n[{tr.side_effect}]"
        g.edge(tr.src, tr.dst, label=label, fontsize="8", color="#475569")

    path = out_dir / "lifecycle"
    g.render(str(path), cleanup=True)
    return path.with_suffix(".png")


def render_create_to_close_sequence(out_dir: Path) -> Path:
    """Mermaid sequence diagram: full happy path."""
    body = """\
sequenceDiagram
    actor U as Admin (AgentK owner)
    participant FE as Frontend
    participant BE as Backend
    participant RM as Rooms svc
    participant WS as Realtime

    U->>FE: Fill form (title / time / editors / viewers)
    FE->>BE: POST /workspaces/{w}/meetings  (status=draft)
    BE->>RM: Create room.type="meeting"
    BE->>BE: INSERT meetings + meeting_participants
    BE->>RM: Sync participants → room_members
    BE-->>WS: emit meeting.created
    BE-->>FE: 201 Meeting (with room_id)

    Note over U,FE: ── Pre-meeting: chat / upload prep files inside dedicated room ──

    U->>FE: Click "Schedule"
    FE->>BE: PATCH /meetings/{m} {status:"scheduled"}
    BE->>BE: Validate starts_at + ends_at
    BE->>BE: UPDATE status
    BE-->>WS: emit meeting.scheduled  (full to participants, busy-mask to others)

    Note over U,FE: ── During: conversations / AI commands / record audio in room ──

    U->>FE: Click "End meeting"
    FE->>BE: PATCH /meetings/{m} {status:"ended"}
    BE->>BE: UPDATE status="ended", is_locked=true
    BE-->>WS: emit meeting.ended
    Note right of BE: Room writes now blocked\\n(get_meeting_room_write_block_reason)

    Note over U,FE: ── If reopen needed ──
    U->>FE: Click "Reopen"
    FE->>BE: PATCH /meetings/{m} {is_locked:false}
    Note right of BE: Only admin / workspace.owner allowed
    BE-->>WS: emit meeting.lock_changed
"""
    path = out_dir / "create_to_close.mmd"
    path.write_text(body, encoding="utf-8")
    return path


def render_record_sequence(out_dir: Path) -> Path:
    """Mermaid sequence diagram: AI minutes + task sync."""
    body = """\
sequenceDiagram
    actor U as Admin or Editor
    participant FE as Frontend
    participant BE as Backend
    participant AI as AI Gateway
    participant TS as Tasks svc

    Note over BE: meeting.is_locked must be false

    U->>FE: Click "Generate minutes"
    FE->>BE: POST /meetings/{m}/record/generate
    BE->>BE: Collect transcripts + recent room messages
    BE->>AI: create ai_job(skill="meeting_minutes")
    AI-->>BE: structured {summary, decisions[], action_items[]}
    BE->>BE: UPSERT meeting_records  (preserves old data on AI failure)
    BE-->>FE: MeetingRecord

    U->>FE: Edit action items, assign owners
    FE->>BE: PATCH /meetings/{m}/record

    U->>FE: Click "Sync to tasks"
    FE->>BE: POST /meetings/{m}/record/tasks/sync
    BE->>BE: Pre-validate every assignee is active ws member
    BE->>TS: Create room-scoped tasks  (dedup by title)
    BE->>BE: UPDATE record.task_ids_json
    BE-->>FE: {record, created_tasks[], skipped_existing_titles[]}
"""
    path = out_dir / "record_generation.mmd"
    path.write_text(body, encoding="utf-8")
    return path


def render_permission_matrix(out_dir: Path) -> Path:
    """Role × action allowed/blocked matrix as markdown."""
    cols = PERMISSION_ROLES
    header = "| Action | " + " | ".join(cols) + " |"
    sep = "|" + "|".join(["---"] * (len(cols) + 1)) + "|"
    lines = [
        "# Permission matrix (Kway Dev role names)",
        "",
        "Legend: **Y** allowed · **N** blocked · **L** blocked when "
        "`is_locked = true`",
        "",
        "Kway Dev role mapping reminder:",
        "",
        "| Kway Dev | AgentK | Scope |",
        "| --- | --- | --- |",
        "| workspace.owner | admin | workspace-wide |",
        "| admin | owner | per-meeting |",
        "| editor | delegate | per-meeting |",
        "| viewer | attendee | per-meeting |",
        "",
        header,
        sep,
    ]
    for action, row in PERMISSION_MATRIX.items():
        lines.append("| " + action + " | "
                     + " | ".join(row[c] for c in cols) + " |")
    lines.append("")
    lines.append("Note: `viewer` (AgentK attendee) intentionally cannot "
                 "upload files in the first wave. Kway Dev's existing "
                 "memory-weight policy is preserved — this table is purely "
                 "describing AgentK behavior, not proposing a change.")
    path = out_dir / "permission_matrix.md"
    path.write_text("\n".join(lines), encoding="utf-8")
    return path


def render_api_surface(out_dir: Path) -> Path:
    lines = [
        "# API surface — AgentK meeting endpoints",
        "",
        "All paths nested under `/workspaces/{workspace_id}`.",
        "",
        "| Method | Path | Min role | Purpose |",
        "| --- | --- | --- | --- |",
    ]
    for ep in ENDPOINTS:
        lines.append(f"| {ep.method} | `{ep.path}` | {ep.auth} | {ep.purpose} |")
    lines.append("")
    lines.append("**Notable absence**: there is **no** `DELETE /meetings/{id}`. "
                 "AgentK uses `PATCH status=cancelled` plus a retention worker "
                 "(currently stubbed) for purging. Files use `DELETE` because "
                 "the `assets` table tracks its own 60-day retention.")
    lines.append("")
    lines.append("## Realtime events (over WebSocket)")
    lines.append("")
    lines.append("| Event | Fired by | Audience | Payload |")
    lines.append("| --- | --- | --- | --- |")
    for e in REALTIME_EVENTS:
        lines.append(f"| `{e.name}` | {e.fired_by} | {e.audience} | {e.payload} |")
    path = out_dir / "api_surface.md"
    path.write_text("\n".join(lines), encoding="utf-8")
    return path


def render_role_mapping(out_dir: Path) -> Path:
    lines = [
        "# Role mapping  —  AgentK ↔ Kway Dev",
        "",
        "Kway Dev's existing permission ladder is the canonical naming in "
        "this folder. Memory-weight policy stays unchanged.",
        "",
        "| Authority | AgentK | Kway Dev | Storage |",
        "| --- | --- | --- | --- |",
        "| Highest (workspace-wide) | admin | **owner** | `workspace_members.role` |",
        "| Per-meeting primary | owner | **admin** | `meeting_participants.role='admin'` |",
        "| Per-meeting deputy | delegate | **editor** | `meeting_participants.role='editor'` |",
        "| Per-meeting member | attendee | **viewer** | `meeting_participants.role='viewer'` |",
        "",
        "Reading any AgentK doc, substitute mentally:",
        "",
        "    AgentK 'admin'    → Kway Dev 'owner'",
        "    AgentK 'owner'    → Kway Dev 'admin'",
        "    AgentK 'delegate' → Kway Dev 'editor'",
        "    AgentK 'attendee' → Kway Dev 'viewer'",
        "",
        "When porting AgentK code, rename the string literals on the way "
        "in. The semantic ladder is identical; only the labels swap.",
    ]
    path = out_dir / "role_mapping.md"
    path.write_text("\n".join(lines), encoding="utf-8")
    return path


def render_readme(out_dir: Path, written: list[Path]) -> Path:
    lines = [
        "# AgentK meeting flow — Kway Dev reading guide",
        "",
        "Generated by `render_meeting_flow.py` from the structures defined "
        "in that script. Source of truth in AgentK: "
        "`kway-rdc/AgentK @ agent/leader/workflow` HEAD `3dd5181`.",
        "",
        "## Files",
        "",
    ]
    for p in written:
        lines.append(f"- [{p.name}]({p.name})")
    lines.extend([
        "",
        "## Reading order",
        "",
        "1. `role_mapping.md` — translate AgentK role names to Kway Dev "
        "before reading anything else.",
        "2. `er_diagram.png` — 4-table domain shape (meetings + "
        "meeting_participants + meeting_records + assets).",
        "3. `lifecycle.png` — status × is_locked state machine.",
        "4. `permission_matrix.md` — who can do what (Kway Dev role labels).",
        "5. `api_surface.md` — 14 endpoints + 8 realtime events.",
        "6. `create_to_close.mmd` — full happy-path sequence "
        "(paste into https://mermaid.live to render).",
        "7. `record_generation.mmd` — AI minutes + task-sync sequence.",
        "",
        "## What stays Kway Dev",
        "",
        "- Permission management (memory-weight policy depends on it)",
        "- Existing `meetings` / `meeting_attendees` / `meeting_notes` / "
        "`meeting_task_impacts` tables",
        "- Portal-side reservation (`crm.kway.com.tw`) integration",
        "",
        "## What we want from AgentK",
        "",
        "- Lock state separate from status (cheap structural win)",
        "- Busy masking on calendar / list endpoints",
        "- Records aggregate shape (summary / decisions / action_items / "
        "transcripts refs / ai_jobs refs / task_ids)",
        "- Action-item → task auto sync with dedup",
        "- File soft-delete retention (60-day grace)",
        "- Dedicated room binding (long-term — bigger structural change)",
    ])
    path = out_dir / "README.md"
    path.write_text("\n".join(lines), encoding="utf-8")
    return path


# ─────────────────────────────────────────────────────────────────────────
# Entry point
# ─────────────────────────────────────────────────────────────────────────

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out",
        type=Path,
        default=Path(__file__).parent / "out",
        help="Output directory (default: ./out alongside this script).",
    )
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    written: list[Path] = []
    try:
        written.append(render_er_diagram(args.out))
        written.append(render_lifecycle(args.out))
    except ImportError:
        print("[warn] `graphviz` Python package not installed; skipping "
              "PNG renders. Install with `pip install graphviz` and ensure "
              "the `dot` binary is on PATH.")
    except Exception as exc:  # noqa: BLE001 — surfacing renderer issues
        print(f"[warn] Graphviz render failed: {exc!r}. Make sure "
              "the `dot` binary is installed (e.g. `choco install graphviz` "
              "on Windows).")
    written.append(render_create_to_close_sequence(args.out))
    written.append(render_record_sequence(args.out))
    written.append(render_permission_matrix(args.out))
    written.append(render_api_surface(args.out))
    written.append(render_role_mapping(args.out))
    written.append(render_readme(args.out, written))

    print(f"[done] wrote {len(written)} files to {args.out}")
    for p in written:
        print(f"  - {p.name}")


if __name__ == "__main__":
    main()
