# API surface — AgentK meeting endpoints

All paths nested under `/workspaces/{workspace_id}`.

| Method | Path | Min role | Purpose |
| --- | --- | --- | --- |
| GET | `/workspaces/{wsid}/meetings` | any ws member | List meetings (busy-masked for non-participants) |
| GET | `/workspaces/{wsid}/meetings/calendar` | any ws member | Calendar view; entries flagged visibility=full|busy |
| POST | `/workspaces/{wsid}/meetings` | any ws member | Create meeting + dedicated room in one txn |
| GET | `/workspaces/{wsid}/meetings/{mid}` | viewer+ | Full detail |
| PATCH | `/workspaces/{wsid}/meetings/{mid}` | editor+ | Update metadata / status / lock |
| GET | `/workspaces/{wsid}/meetings/{mid}/record` | viewer+ | Read record aggregate |
| PATCH | `/workspaces/{wsid}/meetings/{mid}/record` | editor+ | Hand-edit summary/decisions/action_items |
| POST | `/workspaces/{wsid}/meetings/{mid}/records/transcripts` | editor+ | Attach transcript entry |
| POST | `/workspaces/{wsid}/meetings/{mid}/record/generate` | editor+ | Trigger AI minutes via ai_jobs |
| POST | `/workspaces/{wsid}/meetings/{mid}/record/tasks/sync` | editor+ | Sync action_items into room tasks (dedup by title) |
| GET | `/workspaces/{wsid}/meetings/{mid}/files` | viewer+ | List files |
| POST | `/workspaces/{wsid}/meetings/{mid}/files` | editor+ | Upload file (writes assets row) |
| GET | `/workspaces/{wsid}/meetings/{mid}/files/{fid}/download` | viewer+ | Download |
| DELETE | `/workspaces/{wsid}/meetings/{mid}/files/{fid}` | editor+ | Soft-delete (60-day retention) |

**Notable absence**: there is **no** `DELETE /meetings/{id}`. AgentK uses `PATCH status=cancelled` plus a retention worker (currently stubbed) for purging. Files use `DELETE` because the `assets` table tracks its own 60-day retention.

## Realtime events (over WebSocket)

| Event | Fired by | Audience | Payload |
| --- | --- | --- | --- |
| `meeting.created` | POST | participants + ws.owner | full meeting payload |
| `meeting.updated` | PATCH metadata | participants + ws.owner | diff fields |
| `meeting.scheduled` | PATCH status=scheduled | participants + ws.owner; busy-masked broadcast to other ws members | full to participants, masked to others |
| `meeting.ended` | PATCH status=ended | participants + ws.owner | meeting id + ended_at |
| `meeting.cancelled` | PATCH status=cancelled | participants + ws.owner | meeting id + cancelled_at |
| `meeting.participants_changed` | PATCH editor/viewer set | participants + ws.owner | before/after participant ids |
| `meeting.record_updated` | any record mutation | participants | record diff |
| `meeting.lock_changed` | PATCH is_locked | participants + ws.owner | {is_locked} |