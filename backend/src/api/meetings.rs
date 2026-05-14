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
    api::{auth::AuthUser, AppState},
    error::{AppError, AppResult},
};
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
    let meetings: Vec<Meeting> = sqlx::query_as(
        // For portal-imported rows the actual booker (e.g. \"張淑芬\") lives
        // in `external_creator_name`; `creator_id` is a fallback system
        // user. Prefer the external name so the sidebar shows who booked
        // the room. App-created meetings have no external name, so we
        // fall through to the joined `users.display_name`.
        "SELECT m.*,
                COALESCE(m.external_creator_name, u.display_name) AS creator_name
         FROM meetings m
         LEFT JOIN users u ON u.id = m.creator_id
         WHERE (
             m.creator_id = $1
             OR EXISTS (
                 SELECT 1 FROM meeting_attendees ma
                 WHERE ma.meeting_id = m.id AND ma.user_id = $1
             )
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
            location, notification_note, status, invitations_sent_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) RETURNING *",
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

    let detail = load_detail(&state, meeting.id, auth_user.id).await?;
    Ok((StatusCode::CREATED, Json(detail)))
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

    sqlx::query(
        "UPDATE meetings SET
            title             = COALESCE($1, title),
            importance        = COALESCE($2, importance),
            start_at          = COALESCE($3, start_at),
            end_at            = COALESCE($4, end_at),
            all_day           = COALESCE($5, all_day),
            recurrence        = COALESCE($6, recurrence),
            timezone          = COALESCE($7, timezone),
            location          = COALESCE($8, location),
            notification_note = COALESCE($9, notification_note),
            status            = COALESCE($10, status),
            project_id        = COALESCE($11, project_id),
            updated_at        = NOW()
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
    Ok(StatusCode::NO_CONTENT)
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
    // Actual email dispatch is deferred to a future mail-service feature;
    // we just flip the status here so the UI reflects the state change.
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
    sqlx::query("DELETE FROM meeting_files WHERE id = $1")
        .bind(file_id)
        .execute(&state.db)
        .await?;
    let _ = fs::remove_file(&file.storage_path);
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

    let inserted: MeetingNotes = sqlx::query_as(
        "INSERT INTO meeting_notes (meeting_id, version, summary, decisions, risks,
            transcript_excerpts, generated_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *",
    )
    .bind(id)
    .bind(next_version)
    .bind(summary)
    .bind(&decisions)
    .bind(&risks)
    .bind(&excerpts)
    .bind(auth_user.id.to_string())
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
請以下方 JSON 結構回覆會議紀錄；只回 JSON，不要額外文字：\n\
```json\n{{\n  \"summary\": \"3-5 句條列重點\",\n\
  \"decisions\": [{{\"text\": \"...\", \"resolved\": true}}],\n\
  \"risks\": [{{\"text\": \"...\", \"severity\": \"high|medium|low\"}}],\n\
  \"transcript_excerpts\": [{{\"speaker\": \"...\", \"time\": \"09:42\", \"content\": \"...\"}}],\n\
  \"task_impacts\": [{{\"impact_type\": \"new|update|progress\", \"description\": \"...\"}}]\n}}\n```",
        meeting.title,
        meeting.start_at.format("%Y-%m-%d %H:%M"),
        attendees_str,
        transcript,
    );

    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: "你是會議記錄助手，依據逐字稿輸出結構化 JSON 紀錄。".into(),
        },
        ChatMessage {
            role: "user".into(),
            content: user_prompt,
        },
    ];

    let raw = HermesClient::new(&state.config)
        .chat(messages)
        .await
        .map_err(|e| AppError::Agent(e.to_string()))?;
    let cleaned = strip_json_fences(&raw);

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
            transcript_excerpts, generated_by)
         VALUES ($1, $2, $3, $4, $5, $6, 'ai') RETURNING *",
    )
    .bind(id)
    .bind(next_version)
    .bind(parsed.summary)
    .bind(&decisions_json)
    .bind(&risks_json)
    .bind(&excerpts_json)
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

    Ok(Json(inserted))
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
        "SELECT * FROM meeting_files WHERE meeting_id = $1
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
