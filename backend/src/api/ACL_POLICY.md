# API ACL Policy

This document records the minimum role required for write operations under `backend/src/api/`.
Contributors should treat these as the default baseline when adding or modifying endpoints.

## Project-scoped write operations

| Module | Operation | Minimum role | Notes |
|---|---|---:|---|
| `tasks.rs` | create task | editor | Collaborative project write. |
| `tasks.rs` | update task | editor | Any editor may update project tasks. |
| `tasks.rs` | delete task | editor | Any editor may delete project tasks. |
| `tasks.rs` | create task comment | editor | Comment author recorded on insert. |
| `tasks.rs` | update task comment | editor + comment owner | SQL must enforce `AND user_id = $actor`. |
| `tasks.rs` | delete task comment | editor + comment owner | SQL must enforce `AND user_id = $actor`. |
| `tasks.rs` | dispatch task / task attempts | editor | Starts/changes project execution state. |
| `sprints.rs` | create sprint | editor | Collaborative planning write. |
| `sprints.rs` | update sprint | editor | Collaborative planning write. |
| `sprints.rs` | delete sprint | admin | Destructive project-level planning action. |
| `conversations.rs` | create conversation | editor | Starts a writable chat thread in the project. |
| `conversations.rs` | send message | editor | Produces persisted messages and agent side effects. |
| `conversations.rs` | delete conversation | editor | Deletes a writable chat thread. |
| `conversation_memory.rs` | approve memory candidate | editor | Writes durable project memory. |
| `conversation_memory.rs` | reject memory candidate | editor | Review decision is a write action. |
| `conversation_memory.rs` | bulk approve candidates | editor | Same as single approve; writes durable project memory. |
| `conversation_memory.rs` | bulk reject candidates | editor | Same as single reject. |
| `projects.rs` | create/update project metadata | owner/admin/editor as implemented | Follow endpoint-specific checks. |
| `access`/ACL endpoints | membership edits | admin or owner | Must remain stricter than normal content writes. |

## User-scoped write operations

| Module | Operation | Minimum role | Notes |
|---|---|---:|---|
| `epics.rs` | all writes | owning user only | Epics are personal cross-project objects, not project ACL objects. |
| `agent_profiles.rs` | all writes | owning user only | Personal agent configuration. |
| `shared_memory.rs` | all writes | owning user only | Personal notes; project visibility is opt-in via `scope_projects`. |

## Read-path guidance

- Project read endpoints generally require `viewer`.
- Write endpoints should never reuse a read-only `viewer` check unless the endpoint is strictly non-mutating.
- Ownership checks are additive: if a write is author-owned (for example task comments), require both the project write role and resource ownership.

## Current audit notes

- `tasks.rs` task-level updates/deletes intentionally allow any `editor` because tasks are collaborative project artifacts.
- `tasks.rs` comment edits/deletes must stay owner-restricted even for editors.
- `conversation_memory.rs` approvals/rejections are durable writes and therefore must require `editor` or above, not `viewer`.
