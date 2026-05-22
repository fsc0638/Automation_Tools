# Permission matrix (Kway Dev role names)

Legend: **Y** allowed · **N** blocked · **L** blocked when `is_locked = true`

Kway Dev role mapping reminder:

| Kway Dev | AgentK | Scope |
| --- | --- | --- |
| workspace.owner | admin | workspace-wide |
| admin | owner | per-meeting |
| editor | delegate | per-meeting |
| viewer | attendee | per-meeting |

| Action | workspace.owner | admin | editor | viewer | non_participant |
|---|---|---|---|---|---|
| View meeting details (full) | Y | Y | Y | Y | N (busy only) |
| Edit meeting metadata | Y | Y | Y | N | N |
| Reassign admin / change owner | Y | Y | N | N | N |
| Add / remove participants | Y | Y | N | N | N |
| End / cancel meeting | Y | Y | Y | N | N |
| Reopen (clear is_locked) | Y | Y | N | N | N |
| Upload file | Y | Y | Y | N | N |
| Download / preview file | Y | Y | Y | Y | N |
| Delete file (soft) | Y | Y | Y | N | N |
| Add transcript entry | Y | Y | Y | N | N |
| Edit record (summary/decisions) | Y | Y | Y | N | N |
| Generate AI minutes | Y | Y | Y | N | N |
| Sync action items → tasks | Y | Y | Y | N | N |
| Send messages in meeting room | L | L | L | L | N |

Note: `viewer` (AgentK attendee) intentionally cannot upload files in the first wave. Kway Dev's existing memory-weight policy is preserved — this table is purely describing AgentK behavior, not proposing a change.