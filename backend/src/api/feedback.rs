use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::post,
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    error::{AppError, AppResult},
};

#[derive(Debug, Serialize, FromRow)]
pub struct MessageFeedback {
    pub id: Uuid,
    pub message_id: Uuid,
    pub user_id: Uuid,
    pub rating: i16,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FeedbackRequest {
    pub rating: i16,
    pub note: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/messages/:id/feedback", post(submit_feedback))
}

async fn submit_feedback(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(message_id): Path<Uuid>,
    Json(req): Json<FeedbackRequest>,
) -> AppResult<(StatusCode, Json<MessageFeedback>)> {
    if req.rating != 1 && req.rating != -1 {
        return Err(AppError::BadRequest("rating must be 1 or -1".into()));
    }

    // Verify ownership transitively: the message must belong to a conversation
    // owned by the requesting user.
    let owns: Option<(Uuid,)> = sqlx::query_as(
        "SELECT m.id FROM messages m
         JOIN conversations c ON c.id = m.conversation_id
         WHERE m.id = $1 AND c.user_id = $2",
    )
    .bind(message_id)
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await?;
    if owns.is_none() {
        return Err(AppError::NotFound("Message not found".into()));
    }

    let row: MessageFeedback = sqlx::query_as(
        "INSERT INTO message_feedback (message_id, user_id, rating, note)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (message_id, user_id) DO UPDATE
         SET rating = EXCLUDED.rating,
             note = EXCLUDED.note,
             updated_at = NOW()
         RETURNING id, message_id, user_id, rating, note",
    )
    .bind(message_id)
    .bind(auth_user.id)
    .bind(req.rating)
    .bind(req.note.as_deref())
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(row)))
}
