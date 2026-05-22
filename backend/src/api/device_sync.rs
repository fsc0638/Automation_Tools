use axum::{
    extract::{Path, Query, State},
    response::Json,
    routing::{get, patch, post},
    Extension, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    error::{AppError, AppResult},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/devices", get(list_devices).post(register_device))
        .route("/devices/:id/heartbeat", post(device_heartbeat))
        .route("/devices/:id/trust", patch(set_device_trust))
        .route("/device-sync/events", get(list_events).post(publish_event))
        .route("/device-sync/events/:id/ack", post(ack_event))
}

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct UserDevice {
    pub id: Uuid,
    pub owner_user_id: Uuid,
    pub display_name: String,
    pub platform: String,
    pub device_public_key: Option<String>,
    pub trusted: bool,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow, Clone)]
pub struct DeviceSyncEventRow {
    pub id: Uuid,
    pub sequence_id: i64,
    pub owner_user_id: Uuid,
    pub source_device_id: Option<Uuid>,
    pub target_device_id: Option<Uuid>,
    pub event_type: String,
    pub payload: Value,
    pub payload_hash: Option<String>,
    pub delivery_state: String,
    pub created_at: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub acked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterDeviceRequest {
    pub display_name: String,
    pub platform: Option<String>,
    pub device_public_key: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct SetTrustRequest {
    pub trusted: bool,
}

#[derive(Debug, Deserialize)]
pub struct PublishEventRequest {
    pub source_device_id: Option<Uuid>,
    pub target_device_id: Option<Uuid>,
    pub event_type: String,
    pub payload: Value,
    pub payload_hash: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ListEventsQuery {
    pub after_sequence_id: Option<i64>,
    pub target_device_id: Option<Uuid>,
    pub limit: Option<i64>,
}

async fn list_devices(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
) -> AppResult<Json<Vec<UserDevice>>> {
    let rows = sqlx::query_as::<_, UserDevice>(
        "SELECT * FROM user_devices WHERE owner_user_id = $1 ORDER BY updated_at DESC",
    )
    .bind(auth_user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn register_device(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<RegisterDeviceRequest>,
) -> AppResult<Json<UserDevice>> {
    let display_name = req.display_name.trim();
    if display_name.is_empty() {
        return Err(AppError::BadRequest("display_name is required".into()));
    }
    let platform = req.platform.unwrap_or_else(|| "unknown".into());
    let metadata = req.metadata.unwrap_or_else(|| serde_json::json!({}));

    let row = sqlx::query_as::<_, UserDevice>(
        "INSERT INTO user_devices (owner_user_id, display_name, platform, device_public_key, metadata, last_seen_at)
         VALUES ($1,$2,$3,$4,$5,NOW())
         ON CONFLICT (owner_user_id, (lower(display_name))) DO UPDATE SET
             platform = EXCLUDED.platform,
             device_public_key = COALESCE(EXCLUDED.device_public_key, user_devices.device_public_key),
             metadata = EXCLUDED.metadata,
             last_seen_at = NOW(),
             updated_at = NOW()
         RETURNING *",
    )
    .bind(auth_user.id)
    .bind(display_name)
    .bind(platform)
    .bind(req.device_public_key)
    .bind(metadata)
    .fetch_one(&state.db)
    .await?;

    audit_device_sync(&state.db, auth_user.id, Some(row.id), None, "register", None).await?;
    Ok(Json(row))
}

async fn device_heartbeat(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<UserDevice>> {
    let row = sqlx::query_as::<_, UserDevice>(
        "UPDATE user_devices SET last_seen_at = NOW(), updated_at = NOW()
         WHERE id = $1 AND owner_user_id = $2
         RETURNING *",
    )
    .bind(id)
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Device not found".into()))?;

    audit_device_sync(&state.db, auth_user.id, Some(id), None, "heartbeat", None).await?;
    Ok(Json(row))
}

async fn set_device_trust(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<SetTrustRequest>,
) -> AppResult<Json<UserDevice>> {
    let row = sqlx::query_as::<_, UserDevice>(
        "UPDATE user_devices SET trusted = $3, updated_at = NOW()
         WHERE id = $1 AND owner_user_id = $2
         RETURNING *",
    )
    .bind(id)
    .bind(auth_user.id)
    .bind(req.trusted)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Device not found".into()))?;

    audit_device_sync(&state.db, auth_user.id, Some(id), None, "trust_changed", None).await?;
    Ok(Json(row))
}

async fn list_events(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Query(query): Query<ListEventsQuery>,
) -> AppResult<Json<Vec<DeviceSyncEventRow>>> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let rows = sqlx::query_as::<_, DeviceSyncEventRow>(
        "SELECT * FROM device_sync_events
         WHERE owner_user_id = $1
           AND sequence_id > $2
           AND ($3::uuid IS NULL OR target_device_id = $3 OR target_device_id IS NULL)
         ORDER BY sequence_id ASC
         LIMIT $4",
    )
    .bind(auth_user.id)
    .bind(query.after_sequence_id.unwrap_or(0))
    .bind(query.target_device_id)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn publish_event(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<PublishEventRequest>,
) -> AppResult<Json<DeviceSyncEventRow>> {
    if let Some(device_id) = req.source_device_id.or(req.target_device_id) {
        ensure_device_owner(&state.db, auth_user.id, device_id).await?;
    }

    let row = sqlx::query_as::<_, DeviceSyncEventRow>(
        "INSERT INTO device_sync_events
            (owner_user_id, source_device_id, target_device_id, event_type, payload, payload_hash)
         VALUES ($1,$2,$3,$4,$5,$6)
         RETURNING *",
    )
    .bind(auth_user.id)
    .bind(req.source_device_id)
    .bind(req.target_device_id)
    .bind(req.event_type)
    .bind(req.payload)
    .bind(req.payload_hash)
    .fetch_one(&state.db)
    .await?;

    audit_device_sync(&state.db, auth_user.id, req.source_device_id, Some(row.id), "publish", None).await?;
    let _ = state.device_sync_events.send(DeviceSyncEvent::Published {
        owner_user_id: auth_user.id,
        event: row.clone(),
    });
    Ok(Json(row))
}

async fn ack_event(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> AppResult<Json<DeviceSyncEventRow>> {
    let row = sqlx::query_as::<_, DeviceSyncEventRow>(
        "UPDATE device_sync_events
            SET delivery_state = 'acked', acked_at = NOW()
          WHERE id = $1 AND owner_user_id = $2
          RETURNING *",
    )
    .bind(id)
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Sync event not found".into()))?;

    if let Some(device_id) = row.target_device_id {
        sqlx::query(
            "INSERT INTO device_sync_cursors (device_id, last_sequence_id)
             VALUES ($1,$2)
             ON CONFLICT (device_id) DO UPDATE SET
                 last_sequence_id = GREATEST(device_sync_cursors.last_sequence_id, EXCLUDED.last_sequence_id),
                 updated_at = NOW()",
        )
        .bind(device_id)
        .bind(row.sequence_id)
        .execute(&state.db)
        .await?;
    }

    audit_device_sync(&state.db, auth_user.id, row.target_device_id, Some(id), "ack", None).await?;
    Ok(Json(row))
}

async fn ensure_device_owner(db: &sqlx::PgPool, owner_user_id: Uuid, device_id: Uuid) -> AppResult<()> {
    let exists: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM user_devices WHERE id = $1 AND owner_user_id = $2",
    )
    .bind(device_id)
    .bind(owner_user_id)
    .fetch_optional(db)
    .await?;
    exists.map(|_| ()).ok_or_else(|| AppError::NotFound("Device not found".into()))
}

async fn audit_device_sync(
    db: &sqlx::PgPool,
    owner_user_id: Uuid,
    device_id: Option<Uuid>,
    event_id: Option<Uuid>,
    operation: &str,
    ip_addr: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO device_sync_audit (owner_user_id, device_id, event_id, operation, ip_addr)
         VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(owner_user_id)
    .bind(device_id)
    .bind(event_id)
    .bind(operation)
    .bind(ip_addr)
    .execute(db)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeviceSyncEvent {
    Published {
        owner_user_id: Uuid,
        event: DeviceSyncEventRow,
    },
}
