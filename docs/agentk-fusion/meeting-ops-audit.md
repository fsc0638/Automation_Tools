# Meeting Operations — Cross-Audit (Kway Dev vs AgentK)

Sources:
- AgentK: `kway-rdc/AgentK @ agent/leader/workflow` HEAD `3dd5181`
  - `product/services/backend/app/core/meetings.py`
  - `product/services/backend/app/api/workspaces.py`
- Kway Dev: `fsc0638/Automation_Tools @ fsc` HEAD `8e548f2`
  - `backend/src/api/meetings.rs`

Role names in this doc use **Kway Dev** vocabulary. AgentK roles
substitute: AgentK admin → Kway owner; AgentK owner → Kway admin;
AgentK delegate → Kway editor; AgentK attendee → Kway viewer.

Legend:
- ✅ same behaviour
- ⚠️ similar but with a divergence worth knowing
- ❌ Kway Dev missing what AgentK has
- ➕ Kway Dev has extra capability AgentK lacks
- 🔁 different design with same outcome

---

## 1. Create meeting

| Aspect | AgentK | Kway Dev | Verdict |
|---|---|---|---|
| Endpoint | `POST /workspaces/{w}/meetings` | `POST /meetings` | 🔁 different tenant scope |
| Tenant key | `workspace_id` (FK workspaces) | `organization_id` + optional `project_id` | 🔁 |
| Side effect: room | Creates a dedicated `room.type='meeting'` in the same txn; binds 1:1 via FK | — (no room concept) | ❌ |
| Side effect: room members | `_sync_room_members()` populates room_members from participants | — | ❌ |
| Side effect: system message | Inserts a `Meeting created: <title>` system message into the bound room | — | ❌ |
| Side effect: ws broadcast | `broadcast_meeting_event(..., 'meeting.created')` | `MeetingEvent::Created` over in-process tokio::broadcast → `/ws/meetings` | ✅ |
| Side effect: portal sync | — | `try_portal_book()` to crm.kway.com.tw; on failure rolls back to status='draft' | ➕ |
| Validation: temporal | `_validate_meeting_temporal_fields()` — status='scheduled' must have starts_at + ends_at; status='draft' allows them empty | only `end_at > start_at` | ⚠️ AgentK stricter |
| Validation: participants | `_validate_participant_sets()` rejects owner being also delegate/attendee; dedupes ids | unique-email check only | ⚠️ |
| Validation: active members | `_ensure_active_workspace_users()` rejects deactivated users | not enforced | ❌ |
| Returns | Meeting (with participants flattened) | MeetingDetail (with attendees + files + notes + impacts + linked_project) | ⚠️ AgentK lighter |
| Persists fields | title / description / status / starts_at / ends_at / location / join_url / external_provider / external_event_id / external_event_url / sync_status / created_by / updated_by | all of those (post mig 0032) + importance / all_day / recurrence / timezone / notification_note / project_id / status='draft|scheduled|in_progress|completed|cancelled' / invitations_sent_at / external_id (opaque) / portal_booked_at / portal_book_error / external_creator_name | ➕ Kway has more |

**Notable**: AgentK's "create" is heavier (room + system msg + member sync); Kway's "create" is lighter on collaboration plumbing but heavier on tenant fields + portal integration.

---

## 2. Update meeting

| Aspect | AgentK | Kway Dev | Verdict |
|---|---|---|---|
| Endpoint | `PATCH /workspaces/{w}/meetings/{m}` | `PATCH /meetings/:id` | ✅ |
| Auth gate | `_can_manage_meeting()` — ws.owner OR meeting.admin/editor; raises PermissionError | `require_meeting_access(Edit)` — creator only (no project-admin access for *edit* — only delete/reopen widen) | ⚠️ AgentK gate wider but stricter on role check |
| Participant change gate | Separate `_can_reassign_participants()` — only meeting.admin or ws.owner can change participants; editor can edit other fields but not participants | unified — anyone with Edit can change attendees | ❌ Kway lacks distinction |
| Auto-lock | `if meeting.status in {"ended","cancelled"}: meeting.is_locked = True` (both states) | `if status='completed' AND no explicit is_locked: is_locked=TRUE` (only completed; not cancelled) | ⚠️ MISMATCH — Kway should lock on cancelled too |
| Room rename on title change | `room.name = f"Meeting: {meeting.title}"` | — | ❌ no room |
| Participant re-sync | `_sync_meeting_participants` + `_sync_room_members` | replaces `meeting_attendees` (delete all except creator's row, re-INSERT) | ⚠️ diff impl |
| Field-by-field mutation | Reads `payload.model_fields_set`; only updates fields explicitly sent | COALESCE($n, col) for each updatable col | 🔁 same outcome |
| ws event | `meeting.updated` (+ `meeting.lock_changed` / `meeting.scheduled` / `meeting.ended` / etc. when applicable) | `MeetingEvent::Updated` always; status-specific events when status changes; `LockChanged` if computed_is_locked set | ✅ |
| 409 path | `MeetingPersistenceConflictError → 409` for integrity violations | not modelled — bubbles 500 | ⚠️ |

---

## 3. List / calendar

### 3.1 List meetings

| Aspect | AgentK | Kway Dev |
|---|---|---|
| Endpoint | `GET /workspaces/{w}/meetings` | `GET /meetings` |
| Default scope | Workspace member sees all in workspace; mask for non-participants | After 0032: full + busy across creator/attendee/portal-imported/project-viewer |
| Filter params | (none built into list) | `project_id`, `status`, `from`, `to` |
| Busy masking | Implemented in calendar entries (visibility full|busy) | Implemented in list (post-process blanks title/loc/desc/join_url on visibility='busy') |
| Order | likely by `starts_at` | DESC by `start_at` |
| Includes | participants ids on each | `creator_name` (JOIN), `visibility` flag |

**Mismatch**: AgentK's busy mask lives on calendar; ours lives on list. Functionally equivalent for the UI.

### 3.2 Calendar

| Aspect | AgentK | Kway Dev |
|---|---|---|
| Endpoint | `GET /workspaces/{w}/meetings/calendar` returns `list[MeetingCalendarEntry]` (full rows w/ visibility) | `GET /meetings/calendar?year=&month=` returns `list[CalendarDay]` (count + has_urgent + has_available_slot) |
| Shape | per-meeting entries | per-day aggregate |
| Busy masking | yes, per entry | doesn't apply (aggregate) |

❌ Kway's calendar is a different artefact (day-level aggregate for a month grid). AgentK's is meeting-level for a free time window. Not directly comparable.

---

## 4. View detail

| Aspect | AgentK | Kway Dev |
|---|---|---|
| Endpoint | `GET /workspaces/{w}/meetings/{m}` | `GET /meetings/:id` |
| Auth gate | `_can_view_meeting_details()` — ws.owner OR meeting.admin/editor/viewer | `require_meeting_access(View)` — creator OR attendee; 404 to non-participants |
| Hide vs 404 | If non-participant: NOT FOUND (404)? — based on filter logic, likely | 404 explicitly via `AppError::NotFound("Meeting not found")` |
| Includes | participants flattened on Meeting | MeetingDetail flattens Meeting + attendees + files + latest_notes + task_impacts + linked_project |

**Verdict**: roughly equivalent; Kway's response is heavier in one round-trip (no separate fetch for attendees/files).

---

## 5. Reopen / lock

| Aspect | AgentK | Kway Dev |
|---|---|---|
| How to reopen | `PATCH /meetings/{m}` body `{"is_locked": false}` | Dedicated `POST /meetings/:id/reopen` (no body) |
| Role gate | `_can_manage_meeting` — ws.owner OR meeting.admin/editor; subject to PATCH route's gate | Creator OR project owner/admin; **editor cannot reopen** |
| Behavior | idempotent (no-op if already unlocked) | idempotent (returns current detail) |
| ws event | `meeting.lock_changed {is_locked:false}` | `MeetingEvent::LockChanged` + `Updated` |

⚠️ **Permission divergence**: AgentK lets editor reopen; Kway only allows admin/owner. Choice on our side was deliberate (memory-weight policy).

---

## 6. Delete

| Aspect | AgentK | Kway Dev |
|---|---|---|
| Endpoint | **does not exist** | `DELETE /meetings/:id` |
| AgentK alt | `PATCH status='cancelled'` + retention timer (stubbed) | physical DELETE |
| Side effect: portal | — | `try_portal_cancel()` to crm.kway.com.tw if portal_booked_at OR external_id non-NULL |
| Side effect: files | retention sweep on the assets table eventually | inline soft-delete columns sweep (mig 0034); DB cascade clears meeting_files rows on meeting delete |
| Role gate | (no delete; cancel via PATCH which uses `_can_manage_meeting`) | Creator OR project owner/admin (wider than Edit) |
| ws event | (no delete event); `meeting.cancelled` only | `MeetingEvent::Deleted` |

🔁 **Different model**: AgentK keeps cancelled rows for audit; we physically delete. Both supported by the same UX action.

---

## 7. Attendee / participant management

| Aspect | AgentK | Kway Dev |
|---|---|---|
| Table | `meeting_participants` (id, meeting_id, user_id, role, created_at, updated_at) | `meeting_attendees` (meeting_id, user_id, email, display_name, role_label, confirmation_status, confirmed_at, dispute_note, last_action_at, created_at) |
| Identity | user_id (FK users) required | user_id nullable; email always present |
| Role enum | strict `owner | delegate | attendee` | free-text `role_label` (used semantically as admin/editor/viewer) |
| External attendees | not supported (user must exist) | supported (email-only attendee, user_id NULL) |
| Confirmation flow | — | `confirmation_status` (pending/confirmed/disputed) + `confirmed_at` + `dispute_note` + endpoints `/attendees/:email/confirm` + `/attendees/:email/dispute` |
| Edit / replace | `_sync_meeting_participants()` — diff against existing, insert/update/delete | replace-all: DELETE all-but-creator, INSERT each email from payload |

➕ Kway has **confirmation/dispute** workflow that AgentK doesn't model.
❌ Kway doesn't enforce typed roles — `role_label` is free text.

---

## 8. Status lifecycle

```
AgentK:   draft → scheduled → ended ⇄ scheduled (via reopen)
              \      ↘
               → cancelled

Kway Dev: draft → scheduled → in_progress → completed ⇄ scheduled (via reopen)
              \      ↘            ↘
               → cancelled    → cancelled
```

| Status | AgentK | Kway Dev | Verdict |
|---|---|---|---|
| draft | ✓ | ✓ | ✅ |
| scheduled | ✓ | ✓ | ✅ |
| in_progress | — | ✓ | ➕ Kway extra |
| ended / completed | `ended` (auto-locks) | `completed` (auto-locks since mig 0032) | ⚠️ different label |
| cancelled | ✓ (auto-locks) | ✓ (does NOT auto-lock currently) | ❌ MISMATCH — should auto-lock on cancelled |

**Action item**: Update auto-lock rule to also fire on `cancelled` to match AgentK semantics.

---

## 9. Files

| Aspect | AgentK | Kway Dev |
|---|---|---|
| Backing table | generic `assets` (asset_kind='meeting_file') | dedicated `meeting_files` |
| Upload endpoint | `POST .../meetings/{m}/files` | `POST .../meetings/:id/files` (multipart) |
| List | `GET .../meetings/{m}/files` | included in `GET .../:id` (MeetingDetail.files) |
| Download | `GET .../files/{fid}/download` | `GET .../meetings/:id/files/:file_id/download` (similar) |
| Delete (soft) | DELETE → status='soft_deleted' + retention timestamps | DELETE → stamp `deleted_at`/`soft_deleted_until`/`hard_delete_after` (post mig 0034) |
| Hard delete worker | retention worker (stubbed) | `meeting_files_retention_sweep_loop` hourly tick |
| Locked guard | `_require_unlocked` blocks upload/delete when meeting locked | not enforced |
| Permission | upload: ws.owner / meeting.admin / editor; delete: same; viewer can only read | upload: any with View; delete: uploader OR creator |

❌ **Kway gap**: locked meeting doesn't block file upload/delete. Easy to add by reusing the lock check.

---

## 10. Notes / Records

### 10.1 Storage model

| Aspect | AgentK | Kway Dev |
|---|---|---|
| Cardinality | 1:1 per meeting (`meeting_records`) | 1:N versioned (`meeting_notes` with `version` int) |
| Mutation | UPDATE same row | INSERT new version each time |
| Fields | summary, decisions_json, action_items_json, transcript_ids_json, ai_job_ids_json, task_ids_json, updated_by | summary, decisions, risks, transcript_excerpts, action_items, ai_job_ids, task_ids, generated_by |
| Risks (severity) | — | ✓ |
| Audit trail | updated_by + updated_at | dedicated `meeting_notes_edits` table |
| Transcripts | refs to `audio_transcripts` table | inline excerpts (text) |

🔁 Different shape, similar coverage. Kway has versioning + risks + edit audit; AgentK has clean ref model.

### 10.2 AI minutes generation

| Aspect | AgentK | Kway Dev |
|---|---|---|
| Endpoint | `POST .../record/generate` | `POST .../notes/generate` |
| Locked guard | `_require_unlocked` (409 if locked) | not enforced ❌ |
| AI job persistence | dedicated `ai_jobs` table; full lifecycle (created → processing → completed/failed); chains record via ai_job_ids | `ai_jobs` table (post mig 0035); simpler lifecycle (insert pending → UPDATE success/failed) |
| Transcript source | reads `audio_transcripts` records for the room | reads `.txt`/`.md` files uploaded to `meeting_files` |
| Failure handling | leaves existing record intact if AI fails | UPDATE ai_jobs row to failed, returns error; previous note version untouched |
| Action items | `_extract_prefixed_items()` regex on transcript ("decision/action/todo/行動項目") + AI's extracted_tasks merged | LLM returns `action_items` in structured JSON |
| Dedup | casefold by title | exact match |
| Provider routing | calls Hermes via `process_db_ai_job` (workspace AI policy aware) | hardcoded HermesClient — no policy routing |

⚠️ Several gaps:
1. ❌ Locked guard missing on generate / record update
2. ⚠️ Dedup uses exact-match, AgentK casefolds
3. ⚠️ Provider routing — we always use Hermes; AgentK respects per-workspace AI policy

### 10.3 Manual record edit

| Aspect | AgentK | Kway Dev |
|---|---|---|
| Endpoint | `PATCH .../record` | `PATCH .../notes` (creates new version) |
| Locked guard | `_require_unlocked` | not enforced ❌ |
| Role gate | `_require_record_manage` — ws.owner/admin/editor | `AccessLevel::Edit` — creator only |

### 10.4 Transcript entry attach

| Aspect | AgentK | Kway Dev |
|---|---|---|
| Endpoint | `POST .../records/transcripts` | — (only file upload of .txt/.md serves this role) |
| Result | links to audio_transcripts row | inline excerpt in notes |

❌ Kway doesn't have a separate "attach transcript" endpoint.

### 10.5 Task sync

| Aspect | AgentK | Kway Dev |
|---|---|---|
| Endpoint | `POST .../record/tasks/sync` | `POST .../notes/sync-tasks` (post commit f2f5de7) |
| Pre-validation | All assignees must be active workspace members; raises before any insert (atomic) | not pre-validated |
| Idempotent dedup | casefold title against existing tasks; existing tasks **also linked back** into record.task_ids_json | exact match against existing titles |
| Result | `{record, created_tasks, skipped_existing_titles}` | `{synced_notes_version, created_task_ids, skipped_existing_titles}` |
| Source attribution | task.source_ai_job_id ← record's last ai_job_id | — |
| Locked guard | `_require_unlocked` | not enforced ❌ |

⚠️ Three gaps:
1. ❌ Not atomic — could partially create tasks if one assignee fails
2. ⚠️ Doesn't link existing tasks back into record (only logs as skipped)
3. ❌ Doesn't carry `source_ai_job_id` into task

---

## 11. Realtime events

| Event | AgentK | Kway Dev |
|---|---|---|
| meeting.created | ✓ | ✓ |
| meeting.updated | ✓ | ✓ |
| meeting.scheduled | ✓ (on status flip) | ✓ |
| meeting.ended | `meeting.ended` | `MeetingEvent::Ended` |
| meeting.cancelled | ✓ | ✓ |
| meeting.lock_changed | ✓ | ✓ |
| meeting.participants_changed | ✓ (separate event) | — covered by Updated |
| meeting.record_updated | ✓ | ✓ |
| meeting.deleted | — (no delete) | ✓ |
| Transport | WS via existing realtime infra; broadcasts to participants + ws.admin | new `/ws/meetings` endpoint; broadcasts to all subscribers (client filters by meeting_id) |
| Backplane | pg LISTEN/NOTIFY | tokio::broadcast (in-process only) |
| Resync hint | lag tolerance unclear | `{type:'resync'}` sentinel on lagged consumer |

⚠️ **Kway gap**: doesn't separately fire `participants_changed` — generic `Updated` covers it. AgentK is more granular.
❌ **Kway scale**: in-process broadcast won't survive multi-node; AgentK uses pg notify which does.

---

## 12. Permission matrix (Kway Dev terminology)

| Action | workspace.owner | admin (creator) | editor | viewer | non-participant |
|---|---|---|---|---|---|
| Create | (any ws member) | n/a | n/a | n/a | n/a |
| View details | ✓ | ✓ | ✓ | ✓ | ❌ (busy mask only) |
| Edit metadata | ✓ | ✓ | AgentK: ✓; Kway: ❌ | ❌ | ❌ |
| Reassign participants | ✓ | ✓ | ❌ | ❌ | ❌ |
| Status: scheduled/ended/cancelled | ✓ | ✓ | AgentK: ✓; Kway: ❌ | ❌ | ❌ |
| Reopen (clear lock) | ✓ | ✓ | AgentK: ✓; Kway: ❌ | ❌ | ❌ |
| Upload file | ✓ | ✓ | ✓ | ❌ | ❌ |
| Download file | ✓ | ✓ | ✓ | ✓ | ❌ |
| Delete file | ✓ | ✓ | AgentK: ✓; Kway uploader-or-creator only | ❌ | ❌ |
| Edit record | ✓ | ✓ | AgentK: ✓; Kway: ❌ | ❌ | ❌ |
| Generate AI minutes | ✓ | ✓ | AgentK: ✓; Kway: ❌ | ❌ | ❌ |
| Sync action_items → tasks | ✓ | ✓ | AgentK: ✓; Kway: ❌ | ❌ | ❌ |
| Confirm/dispute own attendance | n/a | own row | own row | own row | n/a |

⚠️ **Big divergence**: AgentK gives `editor` (≈ delegate) much wider write rights — almost everything except participant reassignment. Kway only lets creator edit. Combined with our memory-weight policy this might be intentional, but worth noting if AgentK's design is the reference.

---

## 13. External integration

| Aspect | AgentK | Kway Dev |
|---|---|---|
| External calendar | reserved fields only (no sync) | reserved fields + active integration |
| Provider semantics | symbolic `external_provider` | same + opaque `external_id` ("portal:CODE-DATE-HHMM") |
| Push to provider | — | `try_portal_book()` on create/send_invitations (Playwright → crm.kway.com.tw) |
| Cancel on provider | — | `try_portal_cancel()` on delete (best-effort, even for scrape-imported rows) |
| Scrape import | — | every 30 min scrape pulls portal occupancy into meetings + `external_creator_name` |
| Email send | reserved (no sender) | reserved (no sender) |
| Notifications | in-app realtime only | in-app realtime + portal-side scheduling | 

➕ **Kway has a fully working portal integration AgentK lacks**. This is our most concrete differentiator.

---

## 14. Summary: what to fix vs leave

### 🔴 Strict mismatches (should consider closing)

1. **Auto-lock on `cancelled`** — currently only fires on `completed`. AgentK locks both ended + cancelled. 5 lines in update_meeting.
2. **Locked guard on record mutations** — `update_notes`, `generate_ai_notes`, `sync_notes_to_tasks` should 409 if meeting locked. Reopen first. ~3 lines each.
3. **Locked guard on file mutations** — upload + delete should 409 if meeting locked.
4. **`scheduled` requires starts_at + ends_at** — temporal validation rule. Easy.
5. **Task sync casefold dedup** — change `existing.contains(title)` to `existing.iter().any(|t| t.eq_ignore_ascii_case(title))`. AgentK semantically same.
6. **Task sync atomic + assignee validation** — pre-validate all assignees before any insert; transaction-wrap.

### 🟡 Design diffs (decide intentionally)

7. **Editor / 委派 cannot edit / reopen / sync** in Kway — narrower than AgentK. Tied to memory-weight policy; **probably intentional**.
8. **Status `in_progress`** — Kway extra. AgentK doesn't model. Harmless extra.
9. **Notes versioning** — Kway versioned; AgentK single row. Different design tradeoffs; Kway's is heavier but auditable.
10. **Calendar shape** — Kway aggregates by day; AgentK returns per-meeting entries with masking. Different artefact.
11. **No dedicated room** — Kway design choice; AgentK whole record/file model is anchored to a room.
12. **External attendees (email-only)** — Kway has them; AgentK requires registered user.
13. **Confirmation/dispute attendance** — Kway has, AgentK doesn't.

### 🟢 Kway-only (keep)

14. Portal integration (book/cancel/scrape)
15. importance / recurrence / all_day / timezone / notification_note
16. invitations_sent_at + send-invitations endpoint
17. external_creator_name + external_id opaque key
18. meeting_files retention sweep hourly worker
19. Email-only attendees + confirmation flow
20. Meeting risks (severity)
21. Notes versioning + meeting_notes_edits audit table
22. meeting_task_impacts (progress_from/to)

### ⚫ AgentK-only (would need new infra)

23. Dedicated room with system messages + member sync
24. Generic assets table (not just meeting_files)
25. Real audio_transcripts table (we store excerpts inline)
26. pg LISTEN/NOTIFY backplane for multi-node WS
27. Workspace AI policy → routing in AI job
28. Workspace.admin scope (we don't have workspace)

---

## Recommended fixes (concrete)

If we want to close the 🔴 strict mismatches without touching permission/memory-weight policy:

```rust
// 1. Auto-lock on cancelled too (meetings.rs update_meeting)
let computed_is_locked: Option<bool> = match (req.status.as_deref(), req.is_locked) {
    (Some("completed") | Some("cancelled"), None) => Some(true),  // <-- add cancelled
    (_, explicit) => explicit,
};

// 2. Locked guard helper
async fn require_unlocked(state: &AppState, id: Uuid) -> AppResult<()> {
    let locked: bool = sqlx::query_scalar("SELECT is_locked FROM meetings WHERE id=$1")
        .bind(id).fetch_one(&state.db).await?;
    if locked { Err(AppError::Conflict("meeting is locked".into())) } else { Ok(()) }
}
// Call at top of update_notes / generate_ai_notes / sync_notes_to_tasks /
// upload_file / delete_file.

// 3. Temporal validation
if matches!(req.status.as_deref(), Some("scheduled"))
    && (req.start_at.is_none() || req.end_at.is_none())
{ return Err(AppError::BadRequest("scheduled meeting requires start_at + end_at".into())); }

// 4. Sync tasks casefold + atomic
// In sync_notes_to_tasks: use eq_ignore_ascii_case; wrap inserts in
// `sqlx::Acquire` transaction.
```

Total estimated work: **half day** to close the 🔴 list completely.

---

## TL;DR

- **Coverage**: Kway has ~80% of AgentK's meeting capability + a working portal integration + several confirmation/audit extras AgentK lacks.
- **Real gaps**: 6 strict behavior mismatches (mostly locked-guards + auto-lock on cancelled + casefold dedup) — half-day fix.
- **Permission divergence is intentional** (memory-weight policy).
- **Structural diffs** (room, assets, audio_transcripts, pg NOTIFY) won't close in this milestone — different architecture choices.
- **Notes versioning + risks + confirmation flow** are Kway value-adds AgentK doesn't have.
