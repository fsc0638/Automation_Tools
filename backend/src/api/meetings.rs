//! Meetings module (P1). CRUD + attendee confirmation + file management.
//! AI notes generation (P5) and task-impact wiring (P6) are deliberately
//! out of scope here; this module only persists the data shape and serves
//! the basic endpoints the UI workbench needs.

use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{delete as http_delete, get, patch as http_patch, post},
    Extension, Router,
};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::FromRow;
use std::{fs, path::PathBuf};
use uuid::Uuid;

use crate::{
    agents::{hermes::HermesClient, openclaw::ChatMessage},
    api::{auth::AuthUser, AppState, MeetingEvent},
    error::{AppError, AppResult},
};

/// Best-effort event publish — failure (no subscribers) is normal and
/// silenced. Wrap every CRUD success path with this.
fn emit_meeting_event(state: &AppState, event: MeetingEvent) {
    let _ = state.meeting_events.send(event);
}
use kway_dev_backend::portal_book::{self, BookOp, BookRequest};
use kway_dev_backend::portal_sync::{self, SyncOptions};

// ─────────────────────────────────────────────────────────────────────────
// Models
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct Meeting {
    pub id: Uuid,
    pub creator_id: Uuid,
    pub organization_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub title: String,
    pub importance: String,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub all_day: bool,
    pub recurrence: String,
    pub timezone: String,
    pub location: Option<String>,
    pub notification_note: Option<String>,
    pub status: String,
    pub invitations_sent_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Joined from `users.display_name` on the list endpoint so the sidebar
    /// can show "建立會議人" without an extra round-trip. Other handlers
    /// that SELECT `m.*` leave this `None` — `#[sqlx(default)]` keeps
    /// FromRow happy in those cases.
    #[sqlx(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator_name: Option<String>,
    /// Timestamp of the successful portal-side `預約會議室` submission.
    /// `None` until the meeting is pushed; cleared if the operator
    /// cancels and re-creates. UI uses this to show a green "已同步至凱衛"
    /// pill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal_booked_at: Option<DateTime<Utc>>,
    /// Last portal-side failure message, if any. Cleared on success. UI
    /// shows this as a red banner so the operator knows why the meeting
    /// is still a draft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal_book_error: Option<String>,

    // ─── AgentK-aligned columns (migration 0032) — see
    //     docs/agentk-fusion/fusion-plan.md §2.1 for the rationale.
    //     Existing notification_note / external_id / portal_* coexist.
    /// Long-form description of the meeting itself (distinct from
    /// `notification_note`, which is the invitation message).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Independent lock flag. Defaults FALSE; flipped TRUE when status
    /// moves to 'completed' or when an admin explicitly locks the
    /// meeting. Reopen flow clears it.
    #[serde(default)]
    pub is_locked: bool,
    /// Online meeting URL (Webex / Teams / Meet).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join_url: Option<String>,
    /// Symbolic external provider (`webex` / `teams` / `meet` /
    /// `kway-portal`). Distinct from the opaque `external_id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_provider: Option<String>,
    /// External system's event id (Webex/Teams return one; KWay portal
    /// doesn't, the column is still reserved).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_event_id: Option<String>,
    /// Clickable URL back to the external provider's event page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_event_url: Option<String>,
    /// Generic sync status (`pending` / `synced` / `failed`). Coexists
    /// with the KWay-specific `portal_booked_at` / `portal_book_error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_status: Option<String>,
    /// Last sync timestamp across any provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_synced_at: Option<DateTime<Utc>>,
    /// Last user to mutate this row (audit; `updated_at` carries the
    /// time, this carries the who).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by_user_id: Option<Uuid>,

    /// AgentK-aligned busy masking flag: `"full"` when the caller is a
    /// participant (creator or attendee) and sees all fields; `"busy"`
    /// when this row is included only as an occupancy hint — fields like
    /// title/location/notification_note/description/join_url are blanked
    /// before serialization. Populated by `list_meetings` only.
    #[sqlx(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
}

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct MeetingAttendee {
    pub meeting_id: Uuid,
    pub user_id: Option<Uuid>,
    pub email: String,
    pub display_name: String,
    pub role_label: Option<String>,
    pub confirmation_status: String,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub dispute_note: Option<String>,
    pub last_action_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct MeetingFile {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub uploader_id: Uuid,
    pub filename: String,
    pub storage_path: String,
    pub file_size: i64,
    pub mime_type: String,
    pub file_category: String,
    pub upload_status: String,
    pub duration_seconds: Option<i32>,
    pub transcript_meta: Option<String>,
    pub created_at: DateTime<Utc>,

    // AgentK-aligned retention (migration 0034). NULL on active rows;
    // populated when DELETE is called via the soft-delete path.
    #[sqlx(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    #[sqlx(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soft_deleted_until: Option<DateTime<Utc>>,
    #[sqlx(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hard_delete_after: Option<DateTime<Utc>>,
    #[sqlx(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,
}

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct MeetingNotes {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub version: i32,
    pub summary: Option<String>,
    pub decisions: JsonValue,
    pub risks: JsonValue,
    pub transcript_excerpts: JsonValue,
    pub generated_by: String,
    pub created_at: DateTime<Utc>,

    // AgentK-aligned record aggregate fields (migration 0033).
    /// `[{title, description?, assignee_user_id?, source?}]`.
    #[sqlx(default)]
    pub action_items: JsonValue,
    /// AI job IDs that produced this note (refs into `ai_jobs`).
    #[sqlx(default)]
    pub ai_job_ids: JsonValue,
    /// `project_tasks` IDs synced from this note's action_items.
    #[sqlx(default)]
    pub task_ids: JsonValue,
}

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct MeetingTaskImpact {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub project_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub impact_type: String,
    pub description: String,
    pub progress_from: Option<i32>,
    pub progress_to: Option<i32>,
    pub is_hidden: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct LinkedProject {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct MeetingDetail {
    #[serde(flatten)]
    pub meeting: Meeting,
    pub attendees: Vec<MeetingAttendee>,
    pub files: Vec<MeetingFile>,
    pub latest_notes: Option<MeetingNotes>,
    pub task_impacts: Vec<MeetingTaskImpact>,
    pub linked_project: Option<LinkedProject>,
}

#[derive(Debug, Serialize)]
pub struct NotesEdit {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub version: i32,
    pub edited_by: Uuid,
    pub editor_name: Option<String>,
    pub edit_summary: String,
    pub snapshot: JsonValue,
    pub created_at: DateTime<Utc>,
}

// ─────────────────────────────────────────────────────────────────────────
// Request / query types
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateMeetingRequest {
    pub title: String,
    #[serde(default = "default_importance")]
    pub importance: String,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default = "default_recurrence")]
    pub recurrence: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    pub location: Option<String>,
    pub notification_note: Option<String>,
    #[serde(default)]
    pub attendee_emails: Vec<String>,
    pub project_id: Option<Uuid>,
    #[serde(default = "default_save_as_draft")]
    pub save_as_draft: bool,

    // AgentK-aligned optional fields (migration 0032). All None by
    // default so existing clients keep working without changes.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub join_url: Option<String>,
    #[serde(default)]
    pub external_provider: Option<String>,
    #[serde(default)]
    pub external_event_id: Option<String>,
    #[serde(default)]
    pub external_event_url: Option<String>,
}

fn default_importance() -> String { "normal".into() }
fn default_recurrence() -> String { "none".into() }
fn default_timezone() -> String { "Asia/Taipei".into() }
fn default_save_as_draft() -> bool { true }

#[derive(Debug, Deserialize)]
pub struct UpdateMeetingRequest {
    pub title: Option<String>,
    pub importance: Option<String>,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    pub all_day: Option<bool>,
    pub recurrence: Option<String>,
    pub timezone: Option<String>,
    pub location: Option<String>,
    pub notification_note: Option<String>,
    pub status: Option<String>,
    pub attendee_emails: Option<Vec<String>>,
    pub project_id: Option<Uuid>,

    // AgentK-aligned optional fields (migration 0032). All None = leave
    // unchanged. is_locked is settable but reopen flow has its own
    // endpoint with role guard — clients shouldn't normally write this
    // directly through PATCH.
    pub description: Option<String>,
    pub join_url: Option<String>,
    pub external_provider: Option<String>,
    pub external_event_id: Option<String>,
    pub external_event_url: Option<String>,
    pub is_locked: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ListMeetingsQuery {
    pub project_id: Option<Uuid>,
    pub status: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CalendarQuery {
    pub year: i32,
    pub month: u32,
}

#[derive(Debug, Deserialize)]
pub struct AvailableSlotsQuery {
    pub date: NaiveDate,
    pub duration_mins: i64,
    /// Comma-separated email list. (Repeating ?emails=a&emails=b also works
    /// via custom deserialization, but for now we go with the simpler form.)
    pub emails: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RoomsAvailableQuery {
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct RoomAvailability {
    pub name: String,
    pub available: bool,
    /// Set when available=false — describes the booking that blocks it,
    /// so the UI can say "1號會議室(8人)・10:00-14:00 黃若瑀" rather than
    /// just disabling the option silently.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_start_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_end_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct DisputeRequest {
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNotesRequest {
    pub summary: Option<String>,
    pub decisions: Option<JsonValue>,
    pub risks: Option<JsonValue>,
    pub transcript_excerpts: Option<JsonValue>,
    pub edit_summary: Option<String>,
    // AgentK-aligned record aggregate fields (migration 0033).
    pub action_items: Option<JsonValue>,
    pub ai_job_ids: Option<JsonValue>,
    pub task_ids: Option<JsonValue>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskImpactRequest {
    pub project_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub impact_type: String,
    pub description: String,
    pub progress_from: Option<i32>,
    pub progress_to: Option<i32>,
    pub is_hidden: Option<bool>,
}

// ─────────────────────────────────────────────────────────────────────────
// Calendar / available-slots response types
// ─────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CalendarDay {
    pub date: NaiveDate,
    pub meeting_count: i64,
    pub has_urgent: bool,
    pub has_available_slot: bool,
}

#[derive(Debug, Serialize)]
pub struct TimeSlot {
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub available_count: i64,
    pub total_attendees: i64,
    pub busy_names: Vec<String>,
}

// ─────────────────────────────────────────────────────────────────────────
// Routes
// ─────────────────────────────────────────────────────────────────────────

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/meetings", get(list_meetings).post(create_meeting))
        .route("/meetings/sync", post(sync_from_portal))
        .route("/meetings/calendar", get(calendar_view))
        .route("/meetings/available-slots", get(find_available_slots))
        .route("/meetings/rooms/available", get(rooms_available))
        .route(
            "/meetings/:id",
            get(get_meeting).patch(update_meeting).delete(delete_meeting),
        )
        .route("/meetings/:id/send-invitations", post(send_invitations))
        .route("/meetings/:id/reopen", post(reopen_meeting))
        .route(
            "/meetings/:id/attendees/:email/confirm",
            http_patch(confirm_attendance),
        )
        .route(
            "/meetings/:id/attendees/:email/dispute",
            http_patch(dispute_attendance),
        )
        .route(
            "/meetings/:id/files",
            post(upload_file).layer(DefaultBodyLimit::max(200 * 1024 * 1024)),
        )
        .route("/meetings/:id/files/:file_id", http_delete(delete_file))
        .route("/meetings/:id/notes", http_patch(update_notes))
        .route("/meetings/:id/notes/generate", post(generate_ai_notes))
        .route("/meetings/:id/notes/sync-tasks", post(sync_notes_to_tasks))
        .route("/meetings/:id/notes/history", get(notes_history))
        .route(
            "/meetings/:id/task-impacts",
            post(add_task_impact),
        )
        .route(
            "/meetings/:id/task-impacts/:impact_id",
            http_delete(delete_task_impact),
        )
}

// ─────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────

async fn list_meetings(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(q): Query<ListMeetingsQuery>,
) -> AppResult<Json<Vec<Meeting>>> {
    // A user can see meetings they created OR are invited to. SQL uses
    // EXISTS instead of JOIN so the SELECT doesn't multiply rows when
    // the user is an attendee.
    // AgentK-aligned busy masking. We return three buckets of rows:
    //   1. visibility='full' — caller is creator or attendee; sees all
    //   2. visibility='busy' — portal-imported or project-accessible
    //      meeting that the caller has no detail right to. Title /
    //      location / notification_note / description / join_url get
    //      blanked below before serialization, but start/end/room slot
    //      remain so the caller can see "this slot is occupied".
    //   3. omitted entirely — neither participant nor in shared scope.
    //
    // The visibility column is computed inline so we don't need a second
    // round-trip. Permission management is unchanged: this widens what's
    // listed, not who can see details.
    let mut meetings: Vec<Meeting> = sqlx::query_as(
        "SELECT m.*,
                COALESCE(m.external_creator_name, u.display_name) AS creator_name,
                CASE
                    WHEN m.creator_id = $1
                         OR EXISTS (SELECT 1 FROM meeting_attendees ma
                                    WHERE ma.meeting_id = m.id AND ma.user_id = $1)
                    THEN 'full'
                    ELSE 'busy'
                END AS visibility
         FROM meetings m
         LEFT JOIN users u ON u.id = m.creator_id
         WHERE (
             -- Full-visibility scope
             m.creator_id = $1
             OR EXISTS (SELECT 1 FROM meeting_attendees ma
                        WHERE ma.meeting_id = m.id AND ma.user_id = $1)
             -- Busy-masked scope: portal scraped rows (org-wide occupancy)
             OR m.external_id IS NOT NULL
             -- Busy-masked scope: meetings inside a project the user can read
             OR (m.project_id IS NOT NULL
                 AND user_can_access_project(m.project_id, $1, 'viewer'))
         )
         AND ($2::uuid IS NULL OR m.project_id = $2)
         AND ($3::text IS NULL OR m.status = $3)
         AND ($4::timestamptz IS NULL OR m.start_at >= $4)
         AND ($5::timestamptz IS NULL OR m.start_at < $5)
         ORDER BY m.start_at DESC",
    )
    .bind(auth_user.id)
    .bind(q.project_id)
    .bind(q.status.as_deref())
    .bind(q.from)
    .bind(q.to)
    .fetch_all(&state.db)
    .await?;

    // Post-process: blank the human-readable fields on busy rows so the
    // serialized JSON doesn't leak details the caller shouldn't see. We
    // keep room slot info (location stays NULL but start/end/status/
    // is_locked etc. remain) — the UI surfaces this as a 「忙碌」 chip.
    for m in &mut meetings {
        if m.visibility.as_deref() == Some("busy") {
            m.title = "(忙碌)".to_string();
            m.notification_note = None;
            m.description = None;
            m.join_url = None;
            m.external_event_url = None;
            // location intentionally retained — the room is occupancy info.
            // creator_name retained too — knowing WHO booked a room is OK
            // (portal page already shows the booker's name to everyone).
        }
    }

    Ok(Json(meetings))
}

async fn create_meeting(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateMeetingRequest>,
) -> AppResult<(StatusCode, Json<MeetingDetail>)> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }
    if req.end_at <= req.start_at {
        return Err(AppError::BadRequest("end_at must be after start_at".into()));
    }
    if !["normal", "important"].contains(&req.importance.as_str()) {
        return Err(AppError::BadRequest("importance must be normal|important".into()));
    }
    if !["none", "daily", "weekly", "monthly"].contains(&req.recurrence.as_str()) {
        return Err(AppError::BadRequest("invalid recurrence".into()));
    }

    let status = if req.save_as_draft { "draft" } else { "scheduled" };
    let invitations_sent_at = if req.save_as_draft { None } else { Some(Utc::now()) };

    // Inherit organization_id from the linked project when present;
    // otherwise leave NULL (personal meeting).
    let organization_id: Option<Uuid> = if let Some(pid) = req.project_id {
        sqlx::query_scalar("SELECT organization_id FROM projects WHERE id = $1")
            .bind(pid)
            .fetch_optional(&state.db)
            .await?
    } else {
        None
    };

    let meeting: Meeting = sqlx::query_as(
        "INSERT INTO meetings (creator_id, organization_id, project_id, title,
            importance, start_at, end_at, all_day, recurrence, timezone,
            location, notification_note, status, invitations_sent_at,
            description, join_url, external_provider, external_event_id,
            external_event_url, updated_by_user_id)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,
                 $15,$16,$17,$18,$19,$20) RETURNING *",
    )
    .bind(auth_user.id)
    .bind(organization_id)
    .bind(req.project_id)
    .bind(title)
    .bind(&req.importance)
    .bind(req.start_at)
    .bind(req.end_at)
    .bind(req.all_day)
    .bind(&req.recurrence)
    .bind(&req.timezone)
    .bind(req.location.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(req.notification_note.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(status)
    .bind(invitations_sent_at)
    .bind(req.description.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(req.join_url.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(req.external_provider.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(req.external_event_id.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(req.external_event_url.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(auth_user.id)
    .fetch_one(&state.db)
    .await?;

    // Auto-add creator as confirmed attendee so they show up in the
    // signoff list without a separate UX step.
    let creator: Option<(String, String)> = sqlx::query_as(
        "SELECT email, display_name FROM users WHERE id = $1",
    )
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await?;
    if let Some((email, name)) = creator {
        upsert_attendee(&state, meeting.id, Some(auth_user.id), &email, &name, true).await?;
    }

    for raw_email in req.attendee_emails {
        let email = raw_email.trim().to_lowercase();
        if email.is_empty() {
            continue;
        }
        // Resolve display name with a 3-step fallback so attendees imported
        // from the KWay portal show their real Chinese name even before
        // they've ever signed into this app:
        //   1. exact match in `users` (registered local user)
        //   2. case-insensitive match in `portal_employees.email`
        //   3. raw email as last resort
        // `user_id` is only set in case 1 — portal employees don't have a
        // users row yet, and the attendee will be linked the first time
        // they log in (handled elsewhere by the user-onboarding flow).
        let user_row: Option<(Uuid, String)> =
            sqlx::query_as("SELECT id, display_name FROM users WHERE email = $1")
                .bind(&email)
                .fetch_optional(&state.db)
                .await?;
        match user_row {
            Some((uid, name)) => {
                upsert_attendee(&state, meeting.id, Some(uid), &email, &name, false).await?;
            }
            None => {
                let portal_name: Option<String> = sqlx::query_scalar(
                    "SELECT name FROM portal_employees WHERE LOWER(email) = $1 LIMIT 1",
                )
                .bind(&email)
                .fetch_optional(&state.db)
                .await?;
                let display = portal_name.unwrap_or_else(|| email.clone());
                upsert_attendee(&state, meeting.id, None, &email, &display, false).await?;
            }
        }
    }

    // Push the booking to the KWay portal so the room actually shows up
    // reserved in crm.kway.com.tw. We do this synchronously: the operator
    // pressing 送出邀請 expects to know within a few seconds whether the
    // room was taken. Portal failures roll the meeting back to 'draft'
    // with the error stashed in `portal_book_error` — the UI surfaces it
    // so the operator can pick a different room and re-submit.
    //
    // Skip when the meeting was saved as a draft, has no room (online-
    // only), or is already in the past (no point booking history).
    if !req.save_as_draft && meeting.start_at > Utc::now() {
        if let Err(e) = try_portal_book(&state, &meeting).await {
            tracing::warn!("portal book failed for meeting {}: {e:?}", meeting.id);
        }
    }

    emit_meeting_event(&state, MeetingEvent::Created { meeting_id: meeting.id });
    if !req.save_as_draft {
        emit_meeting_event(&state, MeetingEvent::Scheduled { meeting_id: meeting.id });
    }
    let detail = load_detail(&state, meeting.id, auth_user.id).await?;
    Ok((StatusCode::CREATED, Json(detail)))
}

/// Attempt to push a freshly-scheduled meeting to the KWay portal's
/// reservation form. Holds the same `portal_sync_lock` as the scraper so
/// the two automation paths never share a Playwright context. Stamps
/// `portal_booked_at` on success or rolls the meeting back to 'draft'
/// with the error string on failure — the caller never has to think
/// about which DB columns to touch.
async fn try_portal_book(state: &AppState, meeting: &Meeting) -> anyhow::Result<()> {
    let location = meeting.location.as_deref().unwrap_or("").trim();
    if location.is_empty() {
        return Ok(());
    }
    let Some((room_code, room_name)) = portal_book::split_location(Some(location)) else {
        return Ok(());
    };

    // Map our recurrence enum to one of the portal's two forms. Daily /
    // monthly aren't a fit for the portal's weekly-cadence model — we
    // book the first occurrence and leave the rest to a future feature.
    let op = match meeting.recurrence.as_str() {
        "weekly" => BookOp::BookMulti,
        _ => BookOp::Book,
    };

    let date = portal_book::local_date_str(meeting.start_at);
    let req = BookRequest {
        op,
        room_code: room_code.clone(),
        room_name: room_name.clone(),
        date: if op == BookOp::Book { Some(date.clone()) } else { None },
        date_start: if op == BookOp::BookMulti { Some(date.clone()) } else { None },
        // Single-occurrence stand-in: end the recurring window on the same
        // day until we add a recurrence-end column. The portal still
        // creates one booking.
        date_end: if op == BookOp::BookMulti { Some(date.clone()) } else { None },
        period_weeks: 1,
        time_start: portal_book::local_time_str(meeting.start_at),
        time_end: portal_book::local_time_str(meeting.end_at),
        subject: meeting.title.clone(),
    };

    let backend_cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let opts = SyncOptions::from_env(&backend_cwd);
    let _guard = state.portal_sync_lock.0.lock().await;
    let result = portal_book::run(&opts, &req).await;
    drop(_guard);

    match result {
        Ok(r) if r.success => {
            sqlx::query(
                "UPDATE meetings SET
                    portal_booked_at = NOW(),
                    portal_book_error = NULL,
                    updated_at = NOW()
                 WHERE id = $1",
            )
            .bind(meeting.id)
            .execute(&state.db)
            .await?;
            tracing::info!("portal_book: meeting {} reserved on portal", meeting.id);
            Ok(())
        }
        Ok(r) => {
            let err = r.error.unwrap_or_else(|| "portal returned no error tokens but success=false".into());
            sqlx::query(
                "UPDATE meetings SET
                    status = 'draft',
                    portal_book_error = $2,
                    updated_at = NOW()
                 WHERE id = $1",
            )
            .bind(meeting.id)
            .bind(&err)
            .execute(&state.db)
            .await?;
            anyhow::bail!(err);
        }
        Err(e) => {
            let err = format!("subprocess error: {e}");
            sqlx::query(
                "UPDATE meetings SET
                    status = 'draft',
                    portal_book_error = $2,
                    updated_at = NOW()
                 WHERE id = $1",
            )
            .bind(meeting.id)
            .bind(&err)
            .execute(&state.db)
            .await?;
            Err(e)
        }
    }
}

async fn get_meeting(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<MeetingDetail>> {
    require_meeting_access(&state, id, auth_user.id, AccessLevel::View).await?;
    let detail = load_detail(&state, id, auth_user.id).await?;
    Ok(Json(detail))
}

async fn update_meeting(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateMeetingRequest>,
) -> AppResult<Json<MeetingDetail>> {
    require_meeting_access(&state, id, auth_user.id, AccessLevel::Edit).await?;

    if let Some(imp) = req.importance.as_deref() {
        if !["normal", "important"].contains(&imp) {
            return Err(AppError::BadRequest("importance must be normal|important".into()));
        }
    }
    if let Some(rec) = req.recurrence.as_deref() {
        if !["none", "daily", "weekly", "monthly"].contains(&rec) {
            return Err(AppError::BadRequest("invalid recurrence".into()));
        }
    }
    if let Some(st) = req.status.as_deref() {
        if !["draft", "scheduled", "in_progress", "completed", "cancelled"].contains(&st) {
            return Err(AppError::BadRequest("invalid status".into()));
        }
    }

    // AgentK-aligned auto-lock: if the caller is moving status into
    // 'completed' WITHOUT also clearing is_locked explicitly, lock the
    // meeting. This keeps "ended" defaults sensible (ended ⇒ locked) and
    // doesn't override an explicit reopen-then-mark-complete request.
    let computed_is_locked: Option<bool> = match (req.status.as_deref(), req.is_locked) {
        (Some("completed"), None) => Some(true),
        (_, explicit) => explicit,
    };

    sqlx::query(
        "UPDATE meetings SET
            title              = COALESCE($1, title),
            importance         = COALESCE($2, importance),
            start_at           = COALESCE($3, start_at),
            end_at             = COALESCE($4, end_at),
            all_day            = COALESCE($5, all_day),
            recurrence         = COALESCE($6, recurrence),
            timezone           = COALESCE($7, timezone),
            location           = COALESCE($8, location),
            notification_note  = COALESCE($9, notification_note),
            status             = COALESCE($10, status),
            project_id         = COALESCE($11, project_id),
            description        = COALESCE($13, description),
            join_url           = COALESCE($14, join_url),
            external_provider  = COALESCE($15, external_provider),
            external_event_id  = COALESCE($16, external_event_id),
            external_event_url = COALESCE($17, external_event_url),
            is_locked          = COALESCE($18, is_locked),
            updated_by_user_id = $19,
            updated_at         = NOW()
         WHERE id = $12",
    )
    .bind(req.title.as_deref().map(str::trim))
    .bind(req.importance.as_deref())
    .bind(req.start_at)
    .bind(req.end_at)
    .bind(req.all_day)
    .bind(req.recurrence.as_deref())
    .bind(req.timezone.as_deref())
    .bind(req.location.as_deref())
    .bind(req.notification_note.as_deref())
    .bind(req.status.as_deref())
    .bind(req.project_id)
    .bind(id)
    .bind(req.description.as_deref())
    .bind(req.join_url.as_deref())
    .bind(req.external_provider.as_deref())
    .bind(req.external_event_id.as_deref())
    .bind(req.external_event_url.as_deref())
    .bind(computed_is_locked)
    .bind(auth_user.id)
    .execute(&state.db)
    .await?;

    // Replace attendees if the caller sent a new list. Skips the creator
    // row so they're never removed by an over-eager edit.
    if let Some(emails) = req.attendee_emails {
        let creator_email: Option<String> = sqlx::query_scalar(
            "SELECT u.email FROM users u
             JOIN meetings m ON m.creator_id = u.id
             WHERE m.id = $1",
        )
        .bind(id)
        .fetch_optional(&state.db)
        .await?;

        sqlx::query(
            "DELETE FROM meeting_attendees
             WHERE meeting_id = $1
               AND ($2::text IS NULL OR email <> $2)",
        )
        .bind(id)
        .bind(creator_email.as_deref())
        .execute(&state.db)
        .await?;

        for raw in emails {
            let email = raw.trim().to_lowercase();
            if email.is_empty() {
                continue;
            }
            if creator_email.as_deref() == Some(email.as_str()) {
                continue;
            }
            let user_row: Option<(Uuid, String)> =
                sqlx::query_as("SELECT id, display_name FROM users WHERE email = $1")
                    .bind(&email)
                    .fetch_optional(&state.db)
                    .await?;
            match user_row {
                Some((uid, name)) => upsert_attendee(&state, id, Some(uid), &email, &name, false).await?,
                None => upsert_attendee(&state, id, None, &email, &email, false).await?,
            }
        }
    }

    // Emit specific lifecycle events when status / lock changed; always
    // emit a generic Updated as well so subscribers without specific
    // handlers still refetch.
    match req.status.as_deref() {
        Some("scheduled")  => emit_meeting_event(&state, MeetingEvent::Scheduled { meeting_id: id }),
        Some("completed")  => emit_meeting_event(&state, MeetingEvent::Ended     { meeting_id: id }),
        Some("cancelled")  => emit_meeting_event(&state, MeetingEvent::Cancelled { meeting_id: id }),
        _ => {}
    }
    if let Some(locked) = computed_is_locked {
        emit_meeting_event(
            &state,
            MeetingEvent::LockChanged { meeting_id: id, is_locked: locked },
        );
    }
    emit_meeting_event(&state, MeetingEvent::Updated { meeting_id: id });

    let detail = load_detail(&state, id, auth_user.id).await?;
    Ok(Json(detail))
}

async fn delete_meeting(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<StatusCode> {
    // Deletion authority is wider than edit: the creator can always delete,
    // and for project-linked meetings any project-level owner/admin can
    // also delete (so a team lead can clean up after an absent organiser).
    // Standalone meetings remain locked to their creator.
    let row: Option<(Uuid, Option<Uuid>)> =
        sqlx::query_as("SELECT creator_id, project_id FROM meetings WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let (creator_id, project_id) =
        row.ok_or_else(|| AppError::NotFound("Meeting not found".into()))?;
    let allowed = if creator_id == auth_user.id {
        true
    } else if let Some(pid) = project_id {
        sqlx::query_scalar::<_, bool>(
            "SELECT user_can_access_project($1, $2, 'admin')",
        )
        .bind(pid)
        .bind(auth_user.id)
        .fetch_one(&state.db)
        .await?
    } else {
        false
    };
    if !allowed {
        return Err(AppError::Forbidden(
            "Only the meeting creator or a project admin/owner can delete this meeting".into(),
        ));
    }
    // If the meeting was pushed to the KWay portal we have to undo that
    // there first — the DB cascade won't reach across the network. Failure
    // is logged but not fatal: the operator's intent is to remove the
    // local record, and a stranded portal booking can be cleaned up by
    // running the scraper's cancellation tab manually. We bias toward
    // "don't refuse the delete because automation flaked."
    // Fire portal cancel for two kinds of rows:
    //   1. We booked it via the app (portal_booked_at IS NOT NULL)
    //   2. We scraped it from the portal (external_id IS NOT NULL)
    // Case 2 covers meetings the operator wants to clean up that we didn't
    // originally create. The portal will refuse cancel attempts the
    // service account isn't allowed to make — we treat that as a logged
    // warning and proceed with the local delete anyway. Per user
    // 2026-05-15: "(a) + best-effort 失敗就 log".
    let portal_known: Option<(Option<DateTime<Utc>>, Option<String>)> = sqlx::query_as(
        "SELECT portal_booked_at, external_id FROM meetings WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let should_try_cancel = matches!(
        portal_known,
        Some((Some(_), _)) | Some((_, Some(_)))
    );
    if should_try_cancel {
        if let Err(e) = try_portal_cancel(&state, id).await {
            tracing::warn!("portal cancel failed for meeting {id}: {e:?}");
        }
    }

    // Sweep on-disk files first so the DB-level cascade doesn't orphan
    // them. Each remove is best-effort: a missing file shouldn't block the
    // meeting deletion, which is what the user actually wants.
    let file_paths: Vec<(String,)> =
        sqlx::query_as("SELECT storage_path FROM meeting_files WHERE meeting_id = $1")
            .bind(id)
            .fetch_all(&state.db)
            .await?;
    for (path,) in file_paths {
        let _ = fs::remove_file(&path);
    }
    sqlx::query("DELETE FROM meetings WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    emit_meeting_event(&state, MeetingEvent::Deleted { meeting_id: id });
    Ok(StatusCode::NO_CONTENT)
}

async fn try_portal_cancel(state: &AppState, meeting_id: Uuid) -> anyhow::Result<()> {
    let row: Option<(String, String, DateTime<Utc>, DateTime<Utc>, String)> = sqlx::query_as(
        "SELECT title, COALESCE(location, ''), start_at, end_at, recurrence
         FROM meetings WHERE id = $1",
    )
    .bind(meeting_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((_title, location, start_at, end_at, recurrence)) = row else {
        return Ok(());
    };
    let Some((room_code, room_name)) = portal_book::split_location(Some(&location)) else {
        return Ok(());
    };
    let date = portal_book::local_date_str(start_at);
    let req = BookRequest {
        op: BookOp::Cancel,
        room_code,
        room_name,
        date: None,
        date_start: Some(date.clone()),
        date_end: Some(date),
        period_weeks: if recurrence == "weekly" { 1 } else { 1 },
        time_start: portal_book::local_time_str(start_at),
        time_end: portal_book::local_time_str(end_at),
        subject: String::new(),
    };
    let backend_cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let opts = SyncOptions::from_env(&backend_cwd);
    let _guard = state.portal_sync_lock.0.lock().await;
    let r = portal_book::run(&opts, &req).await?;
    drop(_guard);
    if !r.success {
        anyhow::bail!(r.error.unwrap_or_else(|| "cancel returned success=false".into()));
    }
    Ok(())
}

async fn send_invitations(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<MeetingDetail>> {
    require_meeting_access(&state, id, auth_user.id, AccessLevel::Edit).await?;
    sqlx::query(
        "UPDATE meetings SET
            status = 'scheduled',
            invitations_sent_at = NOW(),
            updated_at = NOW()
         WHERE id = $1 AND status = 'draft'",
    )
    .bind(id)
    .execute(&state.db)
    .await?;
    // Try to push to portal once we flip out of draft. Same rules as
    // create_meeting: skip past meetings and online-only meetings. A
    // portal failure rolls the row back to draft so the operator can
    // retry — they won't lose the invitations_sent_at timestamp though,
    // which is fine because the email side is still TODO anyway.
    let meeting: Option<Meeting> = sqlx::query_as("SELECT *, NULL::text AS creator_name FROM meetings WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if let Some(m) = meeting {
        if m.start_at > Utc::now() {
            if let Err(e) = try_portal_book(&state, &m).await {
                tracing::warn!("portal book on send_invitations failed for {id}: {e:?}");
            }
        }
    }
    emit_meeting_event(&state, MeetingEvent::Scheduled { meeting_id: id });
    emit_meeting_event(&state, MeetingEvent::Updated { meeting_id: id });
    // Actual email dispatch is still deferred to a future mail-service
    // feature; we just flip the status + reserve the room here.
    let detail = load_detail(&state, id, auth_user.id).await?;
    Ok(Json(detail))
}

/// POST /meetings/:id/reopen
///
/// Clears `is_locked` so a previously-completed meeting can be edited
/// again. Per docs/agentk-fusion/fusion-plan.md §3.1 — this is the only
/// blessed way out of the auto-lock that fires on status='completed'.
/// Authority matches `delete_meeting`: the meeting creator OR a project-
/// level owner/admin (Kway Dev role names). Workspace-wide reopen by
/// other admins is not modelled yet because the project ACL is the
/// closest analog to AgentK's workspace.admin we have today.
async fn reopen_meeting(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<MeetingDetail>> {
    let row: Option<(Uuid, Option<Uuid>, bool)> = sqlx::query_as(
        "SELECT creator_id, project_id, is_locked FROM meetings WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let (creator_id, project_id, is_locked) =
        row.ok_or_else(|| AppError::NotFound("Meeting not found".into()))?;

    if !is_locked {
        // Idempotent — already unlocked just returns current detail.
        let detail = load_detail(&state, id, auth_user.id).await?;
        return Ok(Json(detail));
    }

    let allowed = if creator_id == auth_user.id {
        true
    } else if let Some(pid) = project_id {
        sqlx::query_scalar::<_, bool>("SELECT user_can_access_project($1, $2, 'admin')")
            .bind(pid)
            .bind(auth_user.id)
            .fetch_one(&state.db)
            .await?
    } else {
        false
    };
    if !allowed {
        return Err(AppError::Forbidden(
            "Only the meeting creator or a project admin/owner can reopen this meeting".into(),
        ));
    }

    sqlx::query(
        "UPDATE meetings SET
            is_locked          = FALSE,
            updated_by_user_id = $2,
            updated_at         = NOW()
         WHERE id = $1",
    )
    .bind(id)
    .bind(auth_user.id)
    .execute(&state.db)
    .await?;

    tracing::info!("meeting {} reopened by user {}", id, auth_user.id);
    emit_meeting_event(
        &state,
        MeetingEvent::LockChanged { meeting_id: id, is_locked: false },
    );
    emit_meeting_event(&state, MeetingEvent::Updated { meeting_id: id });
    let detail = load_detail(&state, id, auth_user.id).await?;
    Ok(Json(detail))
}

async fn confirm_attendance(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((id, email)): Path<(Uuid, String)>,
) -> AppResult<Json<MeetingAttendee>> {
    let email = email.to_lowercase();
    require_attendee_self(&state, id, auth_user.id, &email).await?;
    let row: Option<MeetingAttendee> = sqlx::query_as(
        "UPDATE meeting_attendees SET
            confirmation_status = 'confirmed',
            confirmed_at = NOW(),
            last_action_at = NOW(),
            dispute_note = NULL
         WHERE meeting_id = $1 AND email = $2 RETURNING *",
    )
    .bind(id)
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;
    row.map(Json).ok_or_else(|| AppError::NotFound("Attendee not found".into()))
}

async fn dispute_attendance(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((id, email)): Path<(Uuid, String)>,
    Json(req): Json<DisputeRequest>,
) -> AppResult<Json<MeetingAttendee>> {
    let email = email.to_lowercase();
    require_attendee_self(&state, id, auth_user.id, &email).await?;
    let row: Option<MeetingAttendee> = sqlx::query_as(
        "UPDATE meeting_attendees SET
            confirmation_status = 'disputed',
            last_action_at = NOW(),
            dispute_note = $3
         WHERE meeting_id = $1 AND email = $2 RETURNING *",
    )
    .bind(id)
    .bind(&email)
    .bind(req.note.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .fetch_optional(&state.db)
    .await?;
    row.map(Json).ok_or_else(|| AppError::NotFound("Attendee not found".into()))
}

// ─────────────────────────────────────────────────────────────────────────
// File upload / download
// ─────────────────────────────────────────────────────────────────────────

async fn upload_file(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> AppResult<(StatusCode, Json<MeetingFile>)> {
    require_meeting_access(&state, id, auth_user.id, AccessLevel::View).await?;

    let mut filename: Option<String> = None;
    let mut category: String = "attachment".into();
    let mut bytes: Option<Vec<u8>> = None;
    let mut duration_seconds: Option<i32> = None;
    let mut transcript_meta: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("invalid multipart: {}", e)))?
    {
        let name = field.name().unwrap_or_default().to_string();
        match name.as_str() {
            "file" => {
                filename = field.file_name().map(|s| s.to_string());
                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("file read: {}", e)))?;
                bytes = Some(data.to_vec());
            }
            "category" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                if !["attachment", "recording", "transcript"].contains(&v.as_str()) {
                    return Err(AppError::BadRequest("invalid category".into()));
                }
                category = v;
            }
            "duration_seconds" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                duration_seconds = v.parse().ok();
            }
            "transcript_meta" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(e.to_string()))?;
                if !v.trim().is_empty() {
                    transcript_meta = Some(v);
                }
            }
            _ => {}
        }
    }

    let bytes = bytes.ok_or_else(|| AppError::BadRequest("file is required".into()))?;
    let name = filename
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::BadRequest("filename is required".into()))?
        .to_string();

    let file_id = Uuid::new_v4();
    let dir: PathBuf = PathBuf::from(&state.config.project_data_root)
        .join("meetings")
        .join(id.to_string());
    fs::create_dir_all(&dir).map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    let safe_name = name.replace(['/', '\\'], "_");
    let path = dir.join(format!("{}_{}", file_id, safe_name));
    fs::write(&path, &bytes).map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;

    let mime = guess_mime_from_filename(&name);

    let row: MeetingFile = sqlx::query_as(
        "INSERT INTO meeting_files (id, meeting_id, uploader_id, filename, storage_path,
            file_size, mime_type, file_category, upload_status, duration_seconds, transcript_meta)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'uploaded',$9,$10) RETURNING *",
    )
    .bind(file_id)
    .bind(id)
    .bind(auth_user.id)
    .bind(&name)
    .bind(path.to_string_lossy().to_string())
    .bind(bytes.len() as i64)
    .bind(&mime)
    .bind(&category)
    .bind(duration_seconds)
    .bind(transcript_meta)
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn delete_file(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((id, file_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    require_meeting_access(&state, id, auth_user.id, AccessLevel::View).await?;
    // Uploader-only delete unless the user is the meeting owner.
    let row: Option<MeetingFile> =
        sqlx::query_as("SELECT * FROM meeting_files WHERE id = $1 AND meeting_id = $2")
            .bind(file_id)
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let file = row.ok_or_else(|| AppError::NotFound("File not found".into()))?;
    let creator_id: Option<Uuid> = sqlx::query_scalar("SELECT creator_id FROM meetings WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    if file.uploader_id != auth_user.id && creator_id != Some(auth_user.id) {
        return Err(AppError::Forbidden("Only the uploader or meeting owner can delete this file".into()));
    }
    // AgentK-aligned soft delete (migration 0034): instead of removing
    // the row + on-disk file immediately, stamp the retention timestamps
    // and let the background sweep worker do the real cleanup. Users get
    // a 30-day grace period to recover; the file is physically erased
    // 60 days after `deleted_at`.
    sqlx::query(
        "UPDATE meeting_files SET
            deleted_at         = NOW(),
            soft_deleted_until = NOW() + INTERVAL '30 days',
            hard_delete_after  = NOW() + INTERVAL '60 days'
         WHERE id = $1",
    )
    .bind(file_id)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────
// Notes (manual edit; AI generation lives in P5)
// ─────────────────────────────────────────────────────────────────────────

async fn update_notes(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateNotesRequest>,
) -> AppResult<Json<MeetingNotes>> {
    require_meeting_access(&state, id, auth_user.id, AccessLevel::Edit).await?;

    let prev: Option<MeetingNotes> = sqlx::query_as(
        "SELECT * FROM meeting_notes WHERE meeting_id = $1
         ORDER BY version DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let next_version = prev.as_ref().map(|n| n.version + 1).unwrap_or(1);

    let summary = req.summary.or_else(|| prev.as_ref().and_then(|n| n.summary.clone()));
    let decisions = req
        .decisions
        .or_else(|| prev.as_ref().map(|n| n.decisions.clone()))
        .unwrap_or_else(|| serde_json::json!([]));
    let risks = req
        .risks
        .or_else(|| prev.as_ref().map(|n| n.risks.clone()))
        .unwrap_or_else(|| serde_json::json!([]));
    let excerpts = req
        .transcript_excerpts
        .or_else(|| prev.as_ref().map(|n| n.transcript_excerpts.clone()))
        .unwrap_or_else(|| serde_json::json!([]));
    // AgentK-aligned: action_items / ai_job_ids / task_ids carry forward
    // from prev unless the caller explicitly sends a new value.
    let action_items = req
        .action_items
        .or_else(|| prev.as_ref().map(|n| n.action_items.clone()))
        .unwrap_or_else(|| serde_json::json!([]));
    let ai_job_ids = req
        .ai_job_ids
        .or_else(|| prev.as_ref().map(|n| n.ai_job_ids.clone()))
        .unwrap_or_else(|| serde_json::json!([]));
    let task_ids = req
        .task_ids
        .or_else(|| prev.as_ref().map(|n| n.task_ids.clone()))
        .unwrap_or_else(|| serde_json::json!([]));

    let inserted: MeetingNotes = sqlx::query_as(
        "INSERT INTO meeting_notes (meeting_id, version, summary, decisions, risks,
            transcript_excerpts, generated_by, action_items, ai_job_ids, task_ids)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING *",
    )
    .bind(id)
    .bind(next_version)
    .bind(summary)
    .bind(&decisions)
    .bind(&risks)
    .bind(&excerpts)
    .bind(auth_user.id.to_string())
    .bind(&action_items)
    .bind(&ai_job_ids)
    .bind(&task_ids)
    .fetch_one(&state.db)
    .await?;

    sqlx::query(
        "INSERT INTO meeting_notes_edits (meeting_id, version, edited_by, edit_summary, snapshot)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(next_version)
    .bind(auth_user.id)
    .bind(req.edit_summary.unwrap_or_default())
    .bind(serde_json::to_value(&inserted).unwrap_or(serde_json::json!({})))
    .execute(&state.db)
    .await?;

    emit_meeting_event(&state, MeetingEvent::RecordUpdated { meeting_id: id });
    Ok(Json(inserted))
}

#[derive(Debug, Deserialize, Default)]
struct LlmMeetingNotes {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    decisions: Vec<LlmDecision>,
    #[serde(default)]
    risks: Vec<LlmRisk>,
    #[serde(default)]
    transcript_excerpts: Vec<LlmExcerpt>,
    #[serde(default)]
    task_impacts: Vec<LlmImpactHint>,
    /// AgentK-aligned: structured action items keyed by `title`. The
    /// `decisions` / `risks` lists are about retrospective facts; this
    /// list is forward-looking work — who needs to do what after the
    /// meeting. Synced to project_tasks via a future sync endpoint.
    #[serde(default)]
    action_items: Vec<LlmActionItem>,
}

#[derive(Debug, Deserialize, Default)]
struct LlmActionItem {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: Option<String>,
    /// Free-text name of the person responsible; we don't try to
    /// resolve to a user_id from the LLM output (too lossy). UI shows
    /// it as a string. Future: match to attendees + populate
    /// assignee_user_id.
    #[serde(default)]
    assignee: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct LlmDecision {
    #[serde(default)]
    text: String,
    #[serde(default)]
    resolved: bool,
}

#[derive(Debug, Deserialize, Default)]
struct LlmRisk {
    #[serde(default)]
    text: String,
    #[serde(default)]
    severity: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct LlmExcerpt {
    #[serde(default)]
    speaker: String,
    #[serde(default)]
    time: String,
    #[serde(default)]
    content: String,
}

#[derive(Debug, Deserialize, Default)]
struct LlmImpactHint {
    #[serde(default)]
    impact_type: String,
    #[serde(default)]
    description: String,
}

fn strip_json_fences(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        return rest.trim_end_matches("```").trim().to_string();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        return rest.trim_end_matches("```").trim().to_string();
    }
    trimmed.to_string()
}

/// Read transcript text from files attached to the meeting. Only plain-text
/// transcript files (.txt, .md, application/json) are consumed by the AI step;
/// audio recordings are referenced in the notes_sources metadata but not
/// transcribed here — that requires a separate STT pipeline (out of scope).
async fn collect_transcript_text(
    state: &AppState,
    meeting_id: Uuid,
) -> AppResult<String> {
    let files: Vec<MeetingFile> = sqlx::query_as(
        "SELECT * FROM meeting_files
         WHERE meeting_id = $1
           AND (file_category = 'transcript' OR file_category = 'attachment')
         ORDER BY created_at ASC",
    )
    .bind(meeting_id)
    .fetch_all(&state.db)
    .await?;

    let mut buf = String::new();
    for f in &files {
        let lower = f.filename.to_lowercase();
        let is_text = matches!(f.mime_type.as_str(), "text/plain" | "text/markdown" | "application/json")
            || lower.ends_with(".txt")
            || lower.ends_with(".md");
        if !is_text {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&f.storage_path) {
            buf.push_str(&format!("\n## {}\n{}\n", f.filename, content));
            if buf.len() > 30_000 {
                break;
            }
        }
    }
    Ok(buf)
}

async fn generate_ai_notes(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<MeetingNotes>> {
    require_meeting_access(&state, id, auth_user.id, AccessLevel::Edit).await?;

    let meeting: Meeting = sqlx::query_as("SELECT * FROM meetings WHERE id = $1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    let attendees: Vec<MeetingAttendee> = sqlx::query_as(
        "SELECT * FROM meeting_attendees WHERE meeting_id = $1 ORDER BY created_at ASC",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;

    let transcript = collect_transcript_text(&state, id).await?;
    if transcript.trim().is_empty() {
        return Err(AppError::BadRequest(
            "No transcript/text attachments found. Upload a .txt or .md transcript first.".into(),
        ));
    }

    let attendees_str = attendees
        .iter()
        .map(|a| {
            if let Some(role) = a.role_label.as_deref() {
                format!("{}（{}）", a.display_name, role)
            } else {
                a.display_name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("、");

    let user_prompt = format!(
        "會議名稱：{}\n日期：{}\n與會者：{}\n\n逐字稿與附件：\n{}\n\n---\n\
請以下方 JSON 結構回覆會議紀錄；只回 JSON，不要額外文字。\
所有欄位都是選填，沒有的就回空陣列或空字串：\n\
```json\n{{\n  \"summary\": \"3-5 句條列重點\",\n\
  \"decisions\": [{{\"text\": \"...\", \"resolved\": true}}],\n\
  \"risks\": [{{\"text\": \"...\", \"severity\": \"high|medium|low\"}}],\n\
  \"transcript_excerpts\": [{{\"speaker\": \"...\", \"time\": \"09:42\", \"content\": \"...\"}}],\n\
  \"action_items\": [{{\"title\": \"待辦事項標題\", \"description\": \"細節（選填）\", \"assignee\": \"負責人姓名（從與會者中挑，選填）\"}}],\n\
  \"task_impacts\": [{{\"impact_type\": \"new|update|progress\", \"description\": \"...\"}}]\n}}\n```\n\n\
重點：\n\
- `decisions` 是「已經做出的決策」（過去式）\n\
- `action_items` 是「會後要做的事」（未來式），必須有清楚的可執行動作\n\
- 兩者語意不同，請勿混淆。例如「同意採用方案 A」屬於 decision；\
「下週五前整理出方案 A 的實作計畫」屬於 action_item",
        meeting.title,
        meeting.start_at.format("%Y-%m-%d %H:%M"),
        attendees_str,
        transcript,
    );

    let system_prompt = "你是會議記錄助手，依據逐字稿輸出結構化 JSON 紀錄。";
    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: system_prompt.into(),
        },
        ChatMessage {
            role: "user".into(),
            content: user_prompt.clone(),
        },
    ];

    // AgentK-aligned audit: record an ai_jobs row for every Hermes call.
    // We stamp the row with status='failed' upfront and UPDATE to
    // 'success' once the call completes, so a crashed handler leaves
    // a discoverable failure rather than no record at all.
    let started = std::time::Instant::now();
    let prompt_hash = {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(user_prompt.as_bytes());
        format!("{:x}", h.finalize())
    };
    let input_chars = (system_prompt.len() + user_prompt.len()) as i32;
    let ai_job_id: Uuid = sqlx::query_scalar(
        "INSERT INTO ai_jobs
            (kind, provider, model, requested_by, meeting_id,
             status, input_chars, prompt_hash)
         VALUES ('meeting_minutes', $1, $2, $3, $4,
                 'failed', $5, $6) RETURNING id",
    )
    .bind("hermes")
    .bind(&state.config.hermes_model)
    .bind(auth_user.id)
    .bind(id)
    .bind(input_chars)
    .bind(&prompt_hash)
    .fetch_one(&state.db)
    .await?;

    let raw = match HermesClient::new(&state.config).chat(messages).await {
        Ok(r) => r,
        Err(e) => {
            let err_msg = e.to_string();
            let elapsed = started.elapsed().as_millis() as i32;
            let _ = sqlx::query(
                "UPDATE ai_jobs SET status='failed',
                                     error=$2,
                                     duration_ms=$3
                 WHERE id=$1",
            )
            .bind(ai_job_id)
            .bind(&err_msg)
            .bind(elapsed)
            .execute(&state.db)
            .await;
            return Err(AppError::Agent(err_msg));
        }
    };
    let cleaned = strip_json_fences(&raw);
    let elapsed_ms = started.elapsed().as_millis() as i32;
    let output_chars = raw.len() as i32;
    let _ = sqlx::query(
        "UPDATE ai_jobs SET status='success',
                             output_chars=$2,
                             duration_ms=$3
         WHERE id=$1",
    )
    .bind(ai_job_id)
    .bind(output_chars)
    .bind(elapsed_ms)
    .execute(&state.db)
    .await;

    let parsed: LlmMeetingNotes = serde_json::from_str(&cleaned).unwrap_or_else(|_| LlmMeetingNotes {
        summary: cleaned.clone(),
        ..Default::default()
    });

    let decisions_json = serde_json::json!(parsed
        .decisions
        .iter()
        .map(|d| serde_json::json!({ "text": d.text, "resolved": d.resolved }))
        .collect::<Vec<_>>());
    let risks_json = serde_json::json!(parsed
        .risks
        .iter()
        .map(|r| serde_json::json!({
            "text": r.text,
            "severity": r.severity.clone().unwrap_or_else(|| "medium".into())
        }))
        .collect::<Vec<_>>());
    // AgentK-aligned: best-effort map of assignee free-text name → a
    // local user_id by looking up our attendees first, then falling
    // back to portal_employees. If neither matches we drop user_id and
    // keep the raw name; the UI shows the name regardless.
    let resolve_assignee = |name: &str| -> Option<Uuid> {
        let n = name.trim();
        if n.is_empty() {
            return None;
        }
        attendees.iter().find_map(|a| {
            if a.display_name.trim() == n {
                a.user_id
            } else {
                None
            }
        })
    };
    let mut action_items_value = Vec::new();
    let mut resolve_jobs = Vec::new();
    for ai in &parsed.action_items {
        if ai.title.trim().is_empty() {
            continue;
        }
        let raw_assignee = ai.assignee.as_deref().unwrap_or("").to_string();
        let mut item = serde_json::json!({
            "title": ai.title.trim(),
            "description": ai.description.as_deref().unwrap_or("").to_string(),
            "source": "ai-generated",
        });
        if let Some(uid) = resolve_assignee(&raw_assignee) {
            item["assignee_user_id"] = serde_json::Value::String(uid.to_string());
        }
        if !raw_assignee.is_empty() {
            item["assignee_name"] = serde_json::Value::String(raw_assignee.clone());
        }
        // Defer portal_employees fallback to a SQL pass (next block) so
        // we don't do per-row queries inside the iterator.
        resolve_jobs.push((action_items_value.len(), raw_assignee));
        action_items_value.push(item);
    }
    // Async fallback: for assignees that didn't match an attendee,
    // probe portal_employees by Chinese name and stash the resulting
    // user_id (if the portal employee is linked to a local user). One
    // round-trip per unresolved item — acceptable for the small lists
    // an AI summary produces.
    for (idx, name) in &resolve_jobs {
        let item = &action_items_value[*idx];
        if item.get("assignee_user_id").is_some() {
            continue;
        }
        if name.is_empty() {
            continue;
        }
        let uid: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM portal_employees
             WHERE name = $1 AND user_id IS NOT NULL LIMIT 1",
        )
        .bind(name)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        if let Some(uid) = uid {
            action_items_value[*idx]["assignee_user_id"] =
                serde_json::Value::String(uid.to_string());
        }
    }
    let action_items_json = serde_json::Value::Array(action_items_value);

    let excerpts_json = serde_json::json!(parsed
        .transcript_excerpts
        .iter()
        .map(|x| serde_json::json!({ "speaker": x.speaker, "time": x.time, "content": x.content }))
        .collect::<Vec<_>>());

    let prev_version: Option<i32> = sqlx::query_scalar(
        "SELECT MAX(version) FROM meeting_notes WHERE meeting_id = $1",
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    let next_version = prev_version.unwrap_or(0) + 1;

    let inserted: MeetingNotes = sqlx::query_as(
        "INSERT INTO meeting_notes (meeting_id, version, summary, decisions, risks,
            transcript_excerpts, generated_by, action_items, ai_job_ids, task_ids)
         VALUES ($1, $2, $3, $4, $5, $6, 'ai', $7, $8, $9) RETURNING *",
    )
    .bind(id)
    .bind(next_version)
    .bind(parsed.summary)
    .bind(&decisions_json)
    .bind(&risks_json)
    .bind(&excerpts_json)
    .bind(&action_items_json)
    // ai_job_ids: refs into the `ai_jobs` table (migration 0035). Each
    // entry is the UUID of one chat() call that contributed to this
    // note. Today there's exactly one per generate (the Hermes call
    // above); we keep this as an array so a future multi-step pipeline
    // (e.g. transcribe → summarise → critique) can chain entries.
    .bind(serde_json::json!([ai_job_id.to_string()]))
    // task_ids: populated later by a future POST .../notes/sync-tasks
    // endpoint that turns action_items into project_tasks.
    .bind(serde_json::json!([]))
    .fetch_one(&state.db)
    .await?;

    sqlx::query(
        "INSERT INTO meeting_notes_edits (meeting_id, version, edited_by, edit_summary, snapshot)
         VALUES ($1, $2, $3, 'AI 生成', $4)",
    )
    .bind(id)
    .bind(next_version)
    .bind(auth_user.id)
    .bind(serde_json::to_value(&inserted).unwrap_or(serde_json::json!({})))
    .execute(&state.db)
    .await?;

    // Auto-create task impact rows from the AI hints. We don't try to match
    // task_id here — that's a UX decision left to the user later.
    for hint in parsed.task_impacts {
        if !["new", "update", "progress"].contains(&hint.impact_type.as_str()) {
            continue;
        }
        if hint.description.trim().is_empty() {
            continue;
        }
        let _ = sqlx::query(
            "INSERT INTO meeting_task_impacts
                (meeting_id, project_id, impact_type, description)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(meeting.project_id)
        .bind(&hint.impact_type)
        .bind(hint.description.trim())
        .execute(&state.db)
        .await;
    }

    emit_meeting_event(&state, MeetingEvent::RecordUpdated { meeting_id: id });
    Ok(Json(inserted))
}

/// Response shape for POST /meetings/:id/notes/sync-tasks.
#[derive(Debug, Serialize)]
pub struct SyncTasksResult {
    pub synced_notes_version: i32,
    pub created_task_ids: Vec<Uuid>,
    pub skipped_existing_titles: Vec<String>,
}

/// POST /meetings/:id/notes/sync-tasks
///
/// AgentK-aligned: turn the latest note's `action_items` into rows in
/// `project_tasks`. Idempotent in the sense that a re-run won't double-
/// create — we skip any action item whose title already exists as a
/// task on the same project. Caveat: a meeting without a `project_id`
/// has nowhere to put the tasks, so we 400 in that case rather than
/// silently dropping them.
///
/// Per docs/agentk-fusion/fusion-plan.md §3.2.
async fn sync_notes_to_tasks(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<SyncTasksResult>> {
    require_meeting_access(&state, id, auth_user.id, AccessLevel::Edit).await?;

    let meeting: (Option<Uuid>,) =
        sqlx::query_as("SELECT project_id FROM meetings WHERE id = $1")
            .bind(id)
            .fetch_one(&state.db)
            .await?;
    let Some(project_id) = meeting.0 else {
        return Err(AppError::BadRequest(
            "meeting is not linked to a project; cannot sync action items into tasks".into(),
        ));
    };

    let prev: Option<MeetingNotes> = sqlx::query_as(
        "SELECT * FROM meeting_notes WHERE meeting_id = $1
         ORDER BY version DESC LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?;
    let notes = prev.ok_or_else(|| {
        AppError::BadRequest("no meeting notes to sync; generate or write notes first".into())
    })?;

    // action_items is JSONB — decode into a typed view so we can iterate.
    #[derive(Deserialize, Default)]
    struct ActionItemRow {
        title: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        assignee_user_id: Option<String>,
        #[serde(default)]
        assignee_name: Option<String>,
    }
    let items: Vec<ActionItemRow> =
        serde_json::from_value(notes.action_items.clone()).unwrap_or_default();
    if items.is_empty() {
        return Ok(Json(SyncTasksResult {
            synced_notes_version: notes.version,
            created_task_ids: vec![],
            skipped_existing_titles: vec![],
        }));
    }

    // Pre-fetch the project's existing task titles so we can dedupe in a
    // single round-trip rather than N queries.
    let existing_titles: Vec<String> = sqlx::query_scalar(
        "SELECT title FROM project_tasks WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    let existing: std::collections::HashSet<String> =
        existing_titles.into_iter().collect();

    let mut created: Vec<Uuid> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for it in items {
        let title = it.title.trim();
        if title.is_empty() {
            continue;
        }
        if existing.contains(title) {
            skipped.push(title.to_string());
            continue;
        }
        // Use whichever assignee form we have; assignee column on tasks
        // is plain TEXT so display names are fine, but we prefer a
        // user_id-resolved value when present.
        let assignee = it
            .assignee_name
            .as_deref()
            .or(it.assignee_user_id.as_deref());
        let new_id: Uuid = sqlx::query_scalar(
            "INSERT INTO project_tasks
                (project_id, title, why, assignee, status, priority)
             VALUES ($1, $2, $3, $4, 'todo', 'medium')
             RETURNING id",
        )
        .bind(project_id)
        .bind(title)
        .bind(it.description.trim())
        .bind(assignee)
        .fetch_one(&state.db)
        .await?;
        created.push(new_id);
    }

    // Append the new ids into `task_ids` on the latest notes row so the
    // record aggregate stays the source of truth.
    if !created.is_empty() {
        let mut all_ids: Vec<String> = serde_json::from_value::<Vec<String>>(
            notes.task_ids.clone(),
        )
        .unwrap_or_default();
        for nid in &created {
            all_ids.push(nid.to_string());
        }
        sqlx::query(
            "UPDATE meeting_notes SET task_ids = $1 WHERE id = $2",
        )
        .bind(serde_json::json!(all_ids))
        .bind(notes.id)
        .execute(&state.db)
        .await?;
    }

    if !created.is_empty() {
        emit_meeting_event(&state, MeetingEvent::RecordUpdated { meeting_id: id });
    }
    Ok(Json(SyncTasksResult {
        synced_notes_version: notes.version,
        created_task_ids: created,
        skipped_existing_titles: skipped,
    }))
}

async fn notes_history(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<Vec<NotesEdit>>> {
    require_meeting_access(&state, id, auth_user.id, AccessLevel::View).await?;
    let rows: Vec<(Uuid, Uuid, i32, Uuid, Option<String>, String, JsonValue, DateTime<Utc>)> =
        sqlx::query_as(
            "SELECT e.id, e.meeting_id, e.version, e.edited_by, u.display_name,
                    e.edit_summary, e.snapshot, e.created_at
             FROM meeting_notes_edits e
             LEFT JOIN users u ON u.id = e.edited_by
             WHERE e.meeting_id = $1
             ORDER BY e.created_at DESC",
        )
        .bind(id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, mid, ver, by, name, summary, snap, ts)| NotesEdit {
                id,
                meeting_id: mid,
                version: ver,
                edited_by: by,
                editor_name: name,
                edit_summary: summary,
                snapshot: snap,
                created_at: ts,
            })
            .collect(),
    ))
}

// ─────────────────────────────────────────────────────────────────────────
// Task impacts
// ─────────────────────────────────────────────────────────────────────────

async fn add_task_impact(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<CreateTaskImpactRequest>,
) -> AppResult<(StatusCode, Json<MeetingTaskImpact>)> {
    require_meeting_access(&state, id, auth_user.id, AccessLevel::Edit).await?;
    if !["new", "update", "progress"].contains(&req.impact_type.as_str()) {
        return Err(AppError::BadRequest("impact_type must be new|update|progress".into()));
    }
    let desc = req.description.trim();
    if desc.is_empty() {
        return Err(AppError::BadRequest("description is required".into()));
    }
    let row: MeetingTaskImpact = sqlx::query_as(
        "INSERT INTO meeting_task_impacts
            (meeting_id, project_id, task_id, impact_type, description,
             progress_from, progress_to, is_hidden)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING *",
    )
    .bind(id)
    .bind(req.project_id)
    .bind(req.task_id)
    .bind(&req.impact_type)
    .bind(desc)
    .bind(req.progress_from)
    .bind(req.progress_to)
    .bind(req.is_hidden.unwrap_or(false))
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(row)))
}

async fn delete_task_impact(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((id, impact_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    require_meeting_access(&state, id, auth_user.id, AccessLevel::Edit).await?;
    let result = sqlx::query(
        "DELETE FROM meeting_task_impacts WHERE id = $1 AND meeting_id = $2",
    )
    .bind(impact_id)
    .bind(id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Impact not found".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ─────────────────────────────────────────────────────────────────────────
// Portal sync (manual refresh)
// ─────────────────────────────────────────────────────────────────────────

/// POST /meetings/sync — run one full kway_portal scrape + import pass.
/// Used by the workbench refresh button. Holds the shared portal_sync_lock
/// for the duration, so concurrent clicks (or overlap with the background
/// scheduler) queue rather than overlap.
async fn sync_from_portal(
    State(state): State<AppState>,
    Extension(_auth_user): Extension<AuthUser>,
) -> AppResult<Json<portal_sync::SyncReport>> {
    let backend_cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let opts = SyncOptions::from_env(&backend_cwd);
    let report = portal_sync::run(&state.db, &state.portal_sync_lock, &opts)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    Ok(Json(report))
}

// ─────────────────────────────────────────────────────────────────────────
// Rooms available (for the create-meeting room picker)
// ─────────────────────────────────────────────────────────────────────────

/// GET /meetings/rooms/available?start_at=…&end_at=…
///
/// Returns every known meeting-room name with an `available` flag for the
/// requested window. "Known rooms" comes from distinct `location` values
/// in the meetings table (portal-imported rows plus anything the user has
/// booked in-app). Conflicts are computed against live statuses only —
/// cancelled meetings don't block.
async fn rooms_available(
    State(state): State<AppState>,
    Extension(_auth_user): Extension<AuthUser>,
    Query(q): Query<RoomsAvailableQuery>,
) -> AppResult<Json<Vec<RoomAvailability>>> {
    if q.end_at <= q.start_at {
        return Err(AppError::BadRequest("end_at must be after start_at".into()));
    }

    // Pull known room names (top-N by recency so the list is the rooms
    // people actually use, not every historical stub). LIMIT keeps the
    // result small enough to render in a dropdown.
    let rooms: Vec<(String,)> = sqlx::query_as(
        "SELECT location
         FROM meetings
         WHERE location IS NOT NULL AND location <> ''
         GROUP BY location
         ORDER BY MAX(start_at) DESC
         LIMIT 50",
    )
    .fetch_all(&state.db)
    .await?;

    let mut out: Vec<RoomAvailability> = Vec::with_capacity(rooms.len());
    for (location,) in rooms {
        let conflict: Option<(String, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
            "SELECT title, start_at, end_at FROM meetings
             WHERE location = $1
               AND status IN ('scheduled', 'in_progress')
               AND start_at < $3
               AND end_at > $2
             ORDER BY start_at ASC
             LIMIT 1",
        )
        .bind(&location)
        .bind(q.start_at)
        .bind(q.end_at)
        .fetch_optional(&state.db)
        .await?;

        let (available, ct, cs, ce) = match conflict {
            Some((t, s, e)) => (false, Some(t), Some(s), Some(e)),
            None => (true, None, None, None),
        };
        out.push(RoomAvailability {
            name: location,
            available,
            conflict_title: ct,
            conflict_start_at: cs,
            conflict_end_at: ce,
        });
    }
    // Available ones first, then alphabetical within each group.
    out.sort_by(|a, b| match (a.available, b.available) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    Ok(Json(out))
}

// ─────────────────────────────────────────────────────────────────────────
// Calendar / available-slots
// ─────────────────────────────────────────────────────────────────────────

async fn calendar_view(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(q): Query<CalendarQuery>,
) -> AppResult<Json<Vec<CalendarDay>>> {
    if !(1..=12).contains(&q.month) {
        return Err(AppError::BadRequest("month must be 1..=12".into()));
    }
    let from = Utc
        .with_ymd_and_hms(q.year, q.month, 1, 0, 0, 0)
        .single()
        .ok_or_else(|| AppError::BadRequest("invalid year/month".into()))?;
    let to = if q.month == 12 {
        Utc.with_ymd_and_hms(q.year + 1, 1, 1, 0, 0, 0).single()
    } else {
        Utc.with_ymd_and_hms(q.year, q.month + 1, 1, 0, 0, 0).single()
    }
    .ok_or_else(|| AppError::BadRequest("invalid year/month".into()))?;

    // Cancelled bookings are audit-trail rows (portal re-keys / cancellations)
    // — we keep them in the DB but they shouldn't bump the day's count or
    // dot on the calendar. The /meetings list endpoint still returns them
    // if a caller asks, but this aggregate is for "live" workload only.
    let rows: Vec<(NaiveDate, i64, bool)> = sqlx::query_as(
        "SELECT (m.start_at AT TIME ZONE 'UTC')::date AS day,
                COUNT(*) AS n,
                BOOL_OR(m.importance = 'important') AS urgent
         FROM meetings m
         WHERE m.start_at >= $1 AND m.start_at < $2
           AND m.status <> 'cancelled'
           AND (m.creator_id = $3
                OR EXISTS (SELECT 1 FROM meeting_attendees a
                           WHERE a.meeting_id = m.id AND a.user_id = $3))
         GROUP BY day
         ORDER BY day",
    )
    .bind(from)
    .bind(to)
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|(d, n, urgent)| CalendarDay {
                date: d,
                meeting_count: n,
                has_urgent: urgent,
                has_available_slot: n < 6, // crude heuristic for "still room"
            })
            .collect(),
    ))
}

async fn find_available_slots(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(q): Query<AvailableSlotsQuery>,
) -> AppResult<Json<Vec<TimeSlot>>> {
    if q.duration_mins < 15 || q.duration_mins > 480 {
        return Err(AppError::BadRequest("duration_mins must be in 15..=480".into()));
    }

    let mut emails: Vec<String> = q
        .emails
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(|e| e.trim().to_lowercase())
                .filter(|e| !e.is_empty())
                .collect()
        })
        .unwrap_or_default();
    // Always include the calling user so the schedule is consistent.
    let self_email: Option<String> =
        sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
            .bind(auth_user.id)
            .fetch_optional(&state.db)
            .await?;
    if let Some(e) = self_email {
        if !emails.iter().any(|x| x == &e) {
            emails.push(e);
        }
    }

    let user_rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, email, display_name FROM users WHERE email = ANY($1)",
    )
    .bind(&emails)
    .fetch_all(&state.db)
    .await?;
    let total_attendees = emails.len() as i64;
    let user_ids: Vec<Uuid> = user_rows.iter().map(|(id, _, _)| *id).collect();
    let names_by_id: std::collections::HashMap<Uuid, String> =
        user_rows.iter().map(|(id, _, n)| (*id, n.clone())).collect();

    // Day window 00:00 ~ 23:59 UTC (caller's timezone offset is the UI's job).
    let day_start = q
        .date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| AppError::BadRequest("invalid date".into()))?
        .and_utc();
    let day_end = day_start + Duration::days(1);

    let busy: Vec<(Uuid, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT a.user_id, m.start_at, m.end_at
         FROM meetings m
         JOIN meeting_attendees a ON a.meeting_id = m.id
         WHERE m.status NOT IN ('cancelled')
           AND m.start_at < $1 AND m.end_at > $2
           AND a.user_id = ANY($3)",
    )
    .bind(day_end)
    .bind(day_start)
    .bind(&user_ids)
    .fetch_all(&state.db)
    .await?;

    // Working window 08:00–19:00 with 30-min granularity.
    let work_start = day_start + Duration::hours(8);
    let work_end = day_start + Duration::hours(19);
    let step = Duration::minutes(30);
    let dur = Duration::minutes(q.duration_mins);

    let mut slots = Vec::new();
    let mut cursor = work_start;
    while cursor + dur <= work_end {
        let slot_end = cursor + dur;
        let mut busy_ids: Vec<Uuid> = Vec::new();
        for (uid, s, e) in &busy {
            if *s < slot_end && *e > cursor {
                if !busy_ids.contains(uid) {
                    busy_ids.push(*uid);
                }
            }
        }
        let available = total_attendees - busy_ids.len() as i64;
        let busy_names: Vec<String> = busy_ids
            .iter()
            .filter_map(|id| names_by_id.get(id).cloned())
            .collect();
        slots.push(TimeSlot {
            start_at: cursor,
            end_at: slot_end,
            available_count: available.max(0),
            total_attendees,
            busy_names,
        });
        cursor = cursor + step;
    }

    // Top 5 slots sorted by availability, then by time.
    slots.sort_by(|a, b| {
        b.available_count
            .cmp(&a.available_count)
            .then(a.start_at.cmp(&b.start_at))
    });
    slots.truncate(5);
    slots.sort_by(|a, b| a.start_at.cmp(&b.start_at));

    Ok(Json(slots))
}

// ─────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────

fn guess_mime_from_filename(name: &str) -> String {
    let lower = name.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    match ext {
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        "csv" => "text/csv",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" | "mp4" => "audio/mp4",
        "webm" => "audio/webm",
        "ogg" => "audio/ogg",
        _ => "application/octet-stream",
    }
    .to_string()
}

#[derive(Copy, Clone)]
enum AccessLevel {
    View,
    Edit,
}

async fn require_meeting_access(
    state: &AppState,
    meeting_id: Uuid,
    user_id: Uuid,
    level: AccessLevel,
) -> AppResult<()> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT creator_id FROM meetings WHERE id = $1",
    )
    .bind(meeting_id)
    .fetch_optional(&state.db)
    .await?;
    let creator_id = row
        .ok_or_else(|| AppError::NotFound("Meeting not found".into()))?
        .0;
    if creator_id == user_id {
        return Ok(());
    }
    match level {
        AccessLevel::Edit => Err(AppError::Forbidden(
            "Only the meeting creator can perform this action".into(),
        )),
        AccessLevel::View => {
            let is_attendee: Option<(Uuid,)> = sqlx::query_as(
                "SELECT meeting_id FROM meeting_attendees
                 WHERE meeting_id = $1 AND user_id = $2",
            )
            .bind(meeting_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
            if is_attendee.is_some() {
                Ok(())
            } else {
                Err(AppError::NotFound("Meeting not found".into()))
            }
        }
    }
}

async fn require_attendee_self(
    state: &AppState,
    meeting_id: Uuid,
    user_id: Uuid,
    email: &str,
) -> AppResult<()> {
    let row: Option<(Option<Uuid>, String)> = sqlx::query_as(
        "SELECT a.user_id, u.email
         FROM meeting_attendees a
         JOIN users u ON u.id = $2
         WHERE a.meeting_id = $1 AND a.email = $3",
    )
    .bind(meeting_id)
    .bind(user_id)
    .bind(email)
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some((Some(uid), _)) if uid == user_id => Ok(()),
        Some((_, self_email)) if self_email.to_lowercase() == email.to_lowercase() => Ok(()),
        _ => Err(AppError::Forbidden(
            "You can only confirm or dispute your own attendance".into(),
        )),
    }
}

async fn upsert_attendee(
    state: &AppState,
    meeting_id: Uuid,
    user_id: Option<Uuid>,
    email: &str,
    display_name: &str,
    is_creator: bool,
) -> AppResult<()> {
    let (status, confirmed_at) = if is_creator {
        ("confirmed", Some(Utc::now()))
    } else {
        ("pending", None::<DateTime<Utc>>)
    };
    sqlx::query(
        "INSERT INTO meeting_attendees
            (meeting_id, user_id, email, display_name, confirmation_status, confirmed_at)
         VALUES ($1,$2,$3,$4,$5,$6)
         ON CONFLICT (meeting_id, email)
         DO UPDATE SET
            user_id = EXCLUDED.user_id,
            display_name = EXCLUDED.display_name",
    )
    .bind(meeting_id)
    .bind(user_id)
    .bind(email.to_lowercase())
    .bind(display_name)
    .bind(status)
    .bind(confirmed_at)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn load_detail(
    state: &AppState,
    meeting_id: Uuid,
    _viewer_id: Uuid,
) -> AppResult<MeetingDetail> {
    let meeting: Meeting = sqlx::query_as("SELECT * FROM meetings WHERE id = $1")
        .bind(meeting_id)
        .fetch_one(&state.db)
        .await?;
    let attendees: Vec<MeetingAttendee> = sqlx::query_as(
        "SELECT * FROM meeting_attendees WHERE meeting_id = $1
         ORDER BY created_at ASC",
    )
    .bind(meeting_id)
    .fetch_all(&state.db)
    .await?;
    let files: Vec<MeetingFile> = sqlx::query_as(
        // AgentK-aligned: hide soft-deleted files from the default list.
        // Recovery is a separate (future) endpoint that explicitly asks
        // for `deleted_at IS NOT NULL AND hard_delete_after > NOW()`.
        "SELECT * FROM meeting_files
         WHERE meeting_id = $1 AND deleted_at IS NULL
         ORDER BY created_at DESC",
    )
    .bind(meeting_id)
    .fetch_all(&state.db)
    .await?;
    let latest_notes: Option<MeetingNotes> = sqlx::query_as(
        "SELECT * FROM meeting_notes WHERE meeting_id = $1
         ORDER BY version DESC LIMIT 1",
    )
    .bind(meeting_id)
    .fetch_optional(&state.db)
    .await?;
    let task_impacts: Vec<MeetingTaskImpact> = sqlx::query_as(
        "SELECT * FROM meeting_task_impacts WHERE meeting_id = $1
         ORDER BY created_at DESC",
    )
    .bind(meeting_id)
    .fetch_all(&state.db)
    .await?;
    let linked_project: Option<LinkedProject> = if let Some(pid) = meeting.project_id {
        sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id, name FROM projects WHERE id = $1",
        )
        .bind(pid)
        .fetch_optional(&state.db)
        .await?
        .map(|(id, name)| LinkedProject { id, name })
    } else {
        None
    };

    Ok(MeetingDetail {
        meeting,
        attendees,
        files,
        latest_notes,
        task_impacts,
        linked_project,
    })
}
