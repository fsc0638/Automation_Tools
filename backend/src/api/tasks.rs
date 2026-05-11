use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, patch as http_patch},
    Extension, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    api::{auth::AuthUser, AppState},
    error::{AppError, AppResult},
};

#[derive(Debug, Serialize, FromRow)]
pub struct ProjectTask {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub why: Option<String>,
    pub affected_files: Option<Vec<String>>,
    pub acceptance_criteria: Option<String>,
    pub estimated_effort: Option<String>,
    pub priority: String,
    pub status: String,
    pub source_message_id: Option<Uuid>,
    /// Conversation that contains source_message_id. Resolved via LEFT JOIN
    /// so the frontend can deep-link from a Roadmap card back to the chat.
    pub source_conversation_id: Option<Uuid>,
    // P1 fields
    pub assignee: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub test_plan: Option<String>,
    pub rollback_plan: Option<String>,
    pub definition_of_done: Option<String>,
    /// Always present (NOT NULL DEFAULT '{}') so the frontend can render
    /// chips without a null-check.
    pub labels: Vec<String>,
    // P2 fields
    /// Structured AC: { tests: string[], commands: string[],
    /// diff_hints: string[], behavior: string[] }. NULL means "use the
    /// legacy free-text acceptance_criteria field instead".
    pub acceptance_criteria_v2: Option<JsonValue>,
    pub linked_pr_url: Option<String>,
    pub linked_commit_sha: Option<String>,
    /// IDs of tasks that must be done before this one. Always present
    /// (NOT NULL DEFAULT '{}'); empty array means no dependencies.
    pub depends_on: Vec<Uuid>,
    // P3: sprint binding
    pub sprint_id: Option<Uuid>,
    pub sprint_name: Option<String>,
    /// Count of task_comments. Always present (COALESCE 0). Lets the
    /// Roadmap card show a "💬 N" chip without a per-card fetch.
    pub comment_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const TASK_SELECT: &str = "SELECT t.id, t.project_id, t.title, t.why, t.affected_files,
        t.acceptance_criteria, t.estimated_effort, t.priority, t.status,
        t.source_message_id, m.conversation_id AS source_conversation_id,
        t.assignee, t.due_date, t.test_plan, t.rollback_plan,
        t.definition_of_done, t.labels,
        t.acceptance_criteria_v2, t.linked_pr_url, t.linked_commit_sha,
        t.depends_on,
        t.sprint_id, sp.name AS sprint_name,
        COALESCE(cc.n, 0) AS comment_count,
        t.created_at, t.updated_at
     FROM project_tasks t
     LEFT JOIN messages m ON m.id = t.source_message_id
     LEFT JOIN sprints  sp ON sp.id = t.sprint_id
     LEFT JOIN (
        SELECT task_id, COUNT(*)::int8 AS n FROM task_comments GROUP BY task_id
     ) cc ON cc.task_id = t.id";

#[derive(Debug, Deserialize)]
pub struct CreateTask {
    pub title: String,
    pub why: Option<String>,
    pub affected_files: Option<Vec<String>>,
    pub acceptance_criteria: Option<String>,
    pub estimated_effort: Option<String>,
    pub priority: Option<String>,
    pub source_message_id: Option<Uuid>,
    pub assignee: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub test_plan: Option<String>,
    pub rollback_plan: Option<String>,
    pub definition_of_done: Option<String>,
    pub labels: Option<Vec<String>>,
    pub acceptance_criteria_v2: Option<JsonValue>,
    pub linked_pr_url: Option<String>,
    pub linked_commit_sha: Option<String>,
    pub depends_on: Option<Vec<Uuid>>,
    pub sprint_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTask {
    pub title: Option<String>,
    pub why: Option<String>,
    pub affected_files: Option<Vec<String>>,
    pub acceptance_criteria: Option<String>,
    pub estimated_effort: Option<String>,
    pub priority: Option<String>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub due_date: Option<NaiveDate>,
    pub test_plan: Option<String>,
    pub rollback_plan: Option<String>,
    pub definition_of_done: Option<String>,
    pub labels: Option<Vec<String>>,
    pub acceptance_criteria_v2: Option<JsonValue>,
    pub linked_pr_url: Option<String>,
    pub linked_commit_sha: Option<String>,
    pub depends_on: Option<Vec<Uuid>>,
    /// P3 sprint binding. Send `null` to clear (handled via dedicated
    /// CASE in the update SQL — COALESCE can't represent "clear"). Send
    /// a UUID to set. Omit to leave alone.
    #[serde(default, deserialize_with = "deserialize_some")]
    pub sprint_id: Option<Option<Uuid>>,
    /// Optional explanation attached to a status transition; recorded
    /// in task_status_history. Ignored when status doesn't actually change.
    pub status_note: Option<String>,
}

/// Double-Option helper for serde so we can distinguish "field absent"
/// from "field present with value null". Outer None = absent; Some(None)
/// = explicit null; Some(Some(v)) = value.
fn deserialize_some<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: serde::de::Deserialize<'de>,
    D: serde::de::Deserializer<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Serialize, FromRow)]
pub struct TaskAttempt {
    pub id: Uuid,
    pub task_id: Uuid,
    pub conversation_id: Uuid,
    pub mode: String,
    pub status: String,
    pub dispatched_by: Option<Uuid>,
    pub dispatched_by_name: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct DispatchTask {
    pub mode: String,
    pub note: Option<String>,
    /// Optional pre-existing conversation to attach the attempt to. When
    /// absent, a new conversation is created with the task title.
    pub conversation_id: Option<Uuid>,
    /// Conversation title override. Default: "Task: <task title>".
    pub title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DispatchResult {
    pub attempt: TaskAttempt,
    pub conversation_id: Uuid,
    /// Pre-built prompt the frontend should pre-fill into the composer.
    pub prompt: String,
}

#[derive(Debug, Serialize, FromRow)]
pub struct TaskStatusEvent {
    pub id: Uuid,
    pub task_id: Uuid,
    pub from_status: Option<String>,
    pub to_status: String,
    pub changed_by: Option<Uuid>,
    pub changed_by_name: Option<String>,
    pub note: Option<String>,
    pub changed_at: DateTime<Utc>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/projects/:id/tasks", get(list_tasks).post(create_task))
        .route(
            "/projects/:id/tasks/:task_id",
            http_patch(update_task).delete(delete_task),
        )
        .route(
            "/projects/:id/tasks/:task_id/history",
            get(list_task_history),
        )
        .route(
            "/projects/:id/tasks/:task_id/attempts",
            get(list_task_attempts).post(dispatch_task),
        )
        .route(
            "/projects/:id/tasks/:task_id/audit",
            get(list_task_audit),
        )
        .route(
            "/projects/:id/tasks/:task_id/comments",
            get(list_task_comments).post(create_task_comment),
        )
        .route(
            "/projects/:id/tasks/:task_id/comments/:comment_id",
            http_patch(update_task_comment).delete(delete_task_comment),
        )
}

async fn verify_access(state: &AppState, project_id: Uuid, user_id: Uuid) -> AppResult<()> {
    let exists: Option<(Uuid,)> =
        sqlx::query_as("SELECT id FROM projects WHERE id = $1 AND user_id = $2")
            .bind(project_id)
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("Project not found".into()));
    }
    Ok(())
}

#[derive(Debug, Deserialize, Default)]
pub struct ListTaskQuery {
    /// Filter to a specific sprint. Pass `"none"` to list tasks without a
    /// sprint (backlog). Omit to list everything.
    pub sprint_id: Option<String>,
}

async fn list_tasks(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<ListTaskQuery>,
) -> AppResult<Json<Vec<ProjectTask>>> {
    verify_access(&state, project_id, auth_user.id).await?;

    let (where_extra, sprint_bind): (&str, Option<Uuid>) = match query.sprint_id.as_deref() {
        Some("none") | Some("backlog") => (" AND t.sprint_id IS NULL", None),
        Some(other) if !other.is_empty() => {
            let id = Uuid::parse_str(other)
                .map_err(|_| AppError::BadRequest("invalid sprint_id".into()))?;
            (" AND t.sprint_id = $2", Some(id))
        }
        _ => ("", None),
    };

    let sql = format!(
        "{TASK_SELECT}
         WHERE t.project_id = $1{where_extra}
         ORDER BY
            CASE t.priority WHEN 'critical' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2 ELSE 3 END,
            t.created_at DESC"
    );
    let mut q = sqlx::query_as::<_, ProjectTask>(&sql).bind(project_id);
    if let Some(id) = sprint_bind {
        q = q.bind(id);
    }
    let tasks: Vec<ProjectTask> = q.fetch_all(&state.db).await?;
    Ok(Json(tasks))
}

async fn create_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(project_id): Path<Uuid>,
    Json(req): Json<CreateTask>,
) -> AppResult<(StatusCode, Json<ProjectTask>)> {
    verify_access(&state, project_id, auth_user.id).await?;
    if req.title.trim().is_empty() {
        return Err(AppError::BadRequest("title is required".into()));
    }

    let priority = match req.priority.as_deref() {
        Some(p @ ("low" | "medium" | "high" | "critical")) => p.to_string(),
        _ => "medium".to_string(),
    };

    let labels = req.labels.unwrap_or_default();
    let depends_on = req.depends_on.unwrap_or_default();

    let new_id: (Uuid,) = sqlx::query_as(
        "INSERT INTO project_tasks
         (project_id, title, why, affected_files, acceptance_criteria,
          estimated_effort, priority, source_message_id,
          assignee, due_date, test_plan, rollback_plan, definition_of_done, labels,
          acceptance_criteria_v2, linked_pr_url, linked_commit_sha, depends_on,
          sprint_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                 $15, $16, $17, $18, $19)
         RETURNING id",
    )
    .bind(project_id)
    .bind(req.title.trim())
    .bind(req.why.as_deref())
    .bind(req.affected_files.as_deref())
    .bind(req.acceptance_criteria.as_deref())
    .bind(req.estimated_effort.as_deref())
    .bind(&priority)
    .bind(req.source_message_id)
    .bind(req.assignee.as_deref())
    .bind(req.due_date)
    .bind(req.test_plan.as_deref())
    .bind(req.rollback_plan.as_deref())
    .bind(req.definition_of_done.as_deref())
    .bind(&labels)
    .bind(req.acceptance_criteria_v2.as_ref())
    .bind(req.linked_pr_url.as_deref())
    .bind(req.linked_commit_sha.as_deref())
    .bind(&depends_on)
    .bind(req.sprint_id)
    .fetch_one(&state.db)
    .await?;

    // Initial transition: NULL → "todo" (or whatever was set).
    let _ = sqlx::query(
        "INSERT INTO task_status_history (task_id, from_status, to_status, changed_by, note)
         VALUES ($1, NULL, 'todo', $2, $3)",
    )
    .bind(new_id.0)
    .bind(auth_user.id)
    .bind(Some("created"))
    .execute(&state.db)
    .await;

    let sql = format!("{TASK_SELECT} WHERE t.id = $1");
    let task: ProjectTask = sqlx::query_as(&sql)
        .bind(new_id.0)
        .fetch_one(&state.db)
        .await?;

    Ok((StatusCode::CREATED, Json(task)))
}

async fn update_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateTask>,
) -> AppResult<Json<ProjectTask>> {
    verify_access(&state, project_id, auth_user.id).await?;

    if let Some(s) = req.status.as_deref() {
        if !matches!(s, "todo" | "in-progress" | "done" | "cancelled") {
            return Err(AppError::BadRequest("invalid status".into()));
        }
    }
    if let Some(p) = req.priority.as_deref() {
        if !matches!(p, "low" | "medium" | "high" | "critical") {
            return Err(AppError::BadRequest("invalid priority".into()));
        }
    }

    // Capture the pre-update snapshot of every audited field so we can
    // diff after the UPDATE and write task_audit_log rows for whatever
    // actually changed. status still goes to task_status_history, not
    // here, so we don't duplicate that timeline.
    #[allow(clippy::type_complexity)]
    let prev: Option<(
        String,                 // status (for status_history below)
        Option<Uuid>,           // sprint_id
        Option<String>,         // assignee
        String,                 // priority
        Option<NaiveDate>,      // due_date
        Option<String>,         // linked_pr_url
        Option<String>,         // linked_commit_sha
        Vec<String>,            // labels
        Vec<Uuid>,              // depends_on
    )> = sqlx::query_as(
        "SELECT status, sprint_id, assignee, priority, due_date,
                linked_pr_url, linked_commit_sha, labels, depends_on
         FROM project_tasks WHERE id = $1 AND project_id = $2",
    )
    .bind(task_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    let prev_status = prev.as_ref().map(|p| p.0.clone());
    let prev_snapshot = prev.clone();

    // Reject self-dependency.
    if let Some(deps) = req.depends_on.as_deref() {
        if deps.iter().any(|d| *d == task_id) {
            return Err(AppError::BadRequest("a task cannot depend on itself".into()));
        }
    }

    // sprint_id uses double-Option so the client can distinguish
    // "leave alone" (absent / None) from "clear" (explicit null / Some(None)).
    // The SQL uses CASE WHEN $20 THEN $21 ELSE sprint_id END so $21 may be NULL.
    let sprint_set = req.sprint_id.is_some();
    let sprint_val: Option<Uuid> = req.sprint_id.unwrap_or(None);

    let updated: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE project_tasks SET
            title = COALESCE($1, title),
            why = COALESCE($2, why),
            affected_files = COALESCE($3, affected_files),
            acceptance_criteria = COALESCE($4, acceptance_criteria),
            estimated_effort = COALESCE($5, estimated_effort),
            priority = COALESCE($6, priority),
            status = COALESCE($7, status),
            assignee = COALESCE($8, assignee),
            due_date = COALESCE($9, due_date),
            test_plan = COALESCE($10, test_plan),
            rollback_plan = COALESCE($11, rollback_plan),
            definition_of_done = COALESCE($12, definition_of_done),
            labels = COALESCE($13, labels),
            acceptance_criteria_v2 = COALESCE($14, acceptance_criteria_v2),
            linked_pr_url = COALESCE($15, linked_pr_url),
            linked_commit_sha = COALESCE($16, linked_commit_sha),
            depends_on = COALESCE($17, depends_on),
            sprint_id = CASE WHEN $20 THEN $21 ELSE sprint_id END,
            updated_at = NOW()
         WHERE id = $18 AND project_id = $19
         RETURNING id",
    )
    .bind(req.title.as_deref().map(str::trim))
    .bind(req.why.as_deref())
    .bind(req.affected_files.as_deref())
    .bind(req.acceptance_criteria.as_deref())
    .bind(req.estimated_effort.as_deref())
    .bind(req.priority.as_deref())
    .bind(req.status.as_deref())
    .bind(req.assignee.as_deref())
    .bind(req.due_date)
    .bind(req.test_plan.as_deref())
    .bind(req.rollback_plan.as_deref())
    .bind(req.definition_of_done.as_deref())
    .bind(req.labels.as_deref())
    .bind(req.acceptance_criteria_v2.as_ref())
    .bind(req.linked_pr_url.as_deref())
    .bind(req.linked_commit_sha.as_deref())
    .bind(req.depends_on.as_deref())
    .bind(task_id)
    .bind(project_id)
    .bind(sprint_set)
    .bind(sprint_val)
    .fetch_optional(&state.db)
    .await?;

    let id = updated.ok_or_else(|| AppError::NotFound("Task not found".into()))?.0;

    // Record transition only when status actually changed.
    if let (Some(prev), Some(next)) = (prev_status.as_deref(), req.status.as_deref()) {
        if prev != next {
            let _ = sqlx::query(
                "INSERT INTO task_status_history (task_id, from_status, to_status, changed_by, note)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(id)
            .bind(prev)
            .bind(next)
            .bind(auth_user.id)
            .bind(req.status_note.as_deref())
            .execute(&state.db)
            .await;
        }
    }

    // Audit log for non-status field changes (E1). Best-effort: if any
    // INSERT fails, we don't fail the whole update — the user's edit
    // already landed. status changes are skipped (covered by
    // task_status_history above) to keep history canonical and avoid
    // double-counting in PM rollups.
    if let Some(snap) = prev_snapshot {
        let (_status, p_sprint, p_assignee, p_priority, p_due, p_pr, p_commit, p_labels, p_deps) = snap;

        async fn write_audit(
            db: &sqlx::PgPool,
            task_id: Uuid,
            field: &str,
            old: Option<String>,
            new: Option<String>,
            actor: Uuid,
        ) {
            // Skip writing when both sides are None/None or equal — no real change.
            if old == new { return; }
            let _ = sqlx::query(
                "INSERT INTO task_audit_log (task_id, field, old_value, new_value, actor_id)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(task_id)
            .bind(field)
            .bind(old)
            .bind(new)
            .bind(actor)
            .execute(db)
            .await;
        }

        // sprint_id: double-Option semantics — only audit when client
        // actually sent it. Persist as the UUID string (or empty for
        // "moved to backlog" so old_value distinguishes "was unset").
        if req.sprint_id.is_some() {
            let new_val = sprint_val.map(|u| u.to_string());
            let old_val = p_sprint.map(|u| u.to_string());
            write_audit(&state.db, id, "sprint", old_val, new_val, auth_user.id).await;
        }
        // assignee
        if let Some(new_a) = req.assignee.as_deref() {
            let new_v = if new_a.is_empty() { None } else { Some(new_a.to_string()) };
            write_audit(&state.db, id, "assignee", p_assignee, new_v, auth_user.id).await;
        }
        // priority
        if let Some(new_p) = req.priority.as_deref() {
            if p_priority != new_p {
                write_audit(&state.db, id, "priority", Some(p_priority), Some(new_p.to_string()), auth_user.id).await;
            }
        }
        // due_date
        if let Some(new_d) = req.due_date {
            let new_v = Some(new_d.to_string());
            let old_v = p_due.map(|d| d.to_string());
            write_audit(&state.db, id, "due_date", old_v, new_v, auth_user.id).await;
        }
        // PR url
        if let Some(new_pr) = req.linked_pr_url.as_deref() {
            let new_v = if new_pr.is_empty() { None } else { Some(new_pr.to_string()) };
            write_audit(&state.db, id, "linked_pr_url", p_pr, new_v, auth_user.id).await;
        }
        // commit sha
        if let Some(new_c) = req.linked_commit_sha.as_deref() {
            let new_v = if new_c.is_empty() { None } else { Some(new_c.to_string()) };
            write_audit(&state.db, id, "linked_commit_sha", p_commit, new_v, auth_user.id).await;
        }
        // labels (compare as JSON to keep order-deterministic)
        if let Some(new_l) = req.labels.as_deref() {
            let old_v = Some(serde_json::to_string(&p_labels).unwrap_or_default());
            let new_v = Some(serde_json::to_string(new_l).unwrap_or_default());
            write_audit(&state.db, id, "labels", old_v, new_v, auth_user.id).await;
        }
        // depends_on (JSON array of UUIDs)
        if let Some(new_d) = req.depends_on.as_deref() {
            let old_v = Some(serde_json::to_string(&p_deps).unwrap_or_default());
            let new_v = Some(serde_json::to_string(new_d).unwrap_or_default());
            write_audit(&state.db, id, "depends_on", old_v, new_v, auth_user.id).await;
        }
    }

    let sql = format!("{TASK_SELECT} WHERE t.id = $1");
    let task: ProjectTask = sqlx::query_as(&sql)
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(task))
}

async fn delete_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    verify_access(&state, project_id, auth_user.id).await?;
    let result = sqlx::query("DELETE FROM project_tasks WHERE id = $1 AND project_id = $2")
        .bind(task_id)
        .bind(project_id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Task not found".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Build a structured prompt from a task so an agent has every field
/// (why, AC, test plan, DoD, etc.) in a single message. Mirrors the
/// information a developer/QA reviewer would see in the detail drawer.
fn build_task_prompt(task: &ProjectTask) -> String {
    let mut out = String::new();
    out.push_str("# Task: ");
    out.push_str(&task.title);
    out.push_str("\n\n");
    if let Some(why) = task.why.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("## 為什麼要做\n");
        out.push_str(why.trim());
        out.push_str("\n\n");
    }
    // Collect AC v2 items first so we can tell whether the v2 object has
    // any actual content. An empty {} or {tests:[],commands:[],...} is
    // sent by the frontend whenever the user clears the structured form,
    // and we want those cases to fall through to the legacy free-text AC.
    let ac_v2_sections: Vec<(&str, Vec<&str>)> = task
        .acceptance_criteria_v2
        .as_ref()
        .map(|v| {
            ["tests", "commands", "diff_hints", "behavior"]
                .into_iter()
                .filter_map(|key| {
                    let arr = v.get(key)?.as_array()?;
                    let items: Vec<&str> = arr
                        .iter()
                        .filter_map(|x| x.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .collect();
                    if items.is_empty() { None } else { Some((key, items)) }
                })
                .collect()
        })
        .unwrap_or_default();

    if !ac_v2_sections.is_empty() {
        out.push_str("## 驗收條件 (機器可驗證)\n");
        for (key, items) in ac_v2_sections {
            out.push_str(&format!("**{key}**\n"));
            for it in items {
                out.push_str(&format!("- {it}\n"));
            }
            out.push('\n');
        }
    } else if let Some(ac) = task.acceptance_criteria.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("## 驗收條件\n");
        out.push_str(ac.trim());
        out.push_str("\n\n");
    }
    if let Some(dod) = task.definition_of_done.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("## Definition of Done\n");
        out.push_str(dod.trim());
        out.push_str("\n\n");
    }
    if let Some(test) = task.test_plan.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("## 測試計畫\n");
        out.push_str(test.trim());
        out.push_str("\n\n");
    }
    if let Some(rb) = task.rollback_plan.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str("## 回滾方案\n");
        out.push_str(rb.trim());
        out.push_str("\n\n");
    }
    if let Some(files) = task.affected_files.as_ref().filter(|f| !f.is_empty()) {
        out.push_str("## 受影響檔案\n");
        for f in files {
            out.push_str(&format!("- `{f}`\n"));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "## 元資料\npriority: {} · effort: {} · labels: {}\n",
        task.priority,
        task.estimated_effort.as_deref().unwrap_or("—"),
        if task.labels.is_empty() { "—".into() } else { task.labels.join(", ") },
    ));
    out.push_str(
        "\n請根據此任務開工。若條件或檔案不夠明確，先列出仍需確認的項目，不要直接動程式碼。",
    );
    out
}

async fn dispatch_task(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<DispatchTask>,
) -> AppResult<(StatusCode, Json<DispatchResult>)> {
    verify_access(&state, project_id, auth_user.id).await?;
    let mode_trim = req.mode.trim();
    if mode_trim.is_empty() {
        return Err(AppError::BadRequest("mode is required".into()));
    }
    // Persisted conv.mode is the AgentMode enum; for custom-agent dispatches
    // we still store one of the core modes so downstream queries don't break.
    let conv_mode: &str = if mode_trim.starts_with("agents:") {
        "debate"
    } else if mode_trim.starts_with("agent:") || mode_trim == "openclaw" {
        "openclaw"
    } else if mode_trim == "hermes" {
        "hermes"
    } else if mode_trim == "debate" {
        "debate"
    } else {
        return Err(AppError::BadRequest("unsupported mode".into()));
    };

    // Load the task so we can build the prompt + verify it belongs here.
    let task_sql = format!("{TASK_SELECT} WHERE t.id = $1 AND t.project_id = $2");
    let task: Option<ProjectTask> = sqlx::query_as(&task_sql)
        .bind(task_id)
        .bind(project_id)
        .fetch_optional(&state.db)
        .await?;
    let task = task.ok_or_else(|| AppError::NotFound("Task not found".into()))?;

    // Reuse provided conversation, or create a fresh one tagged for the task.
    let conversation_id = if let Some(existing) = req.conversation_id {
        let exists: Option<(Uuid,)> = sqlx::query_as(
            "SELECT id FROM conversations WHERE id = $1 AND project_id = $2",
        )
        .bind(existing)
        .bind(project_id)
        .fetch_optional(&state.db)
        .await?;
        if exists.is_none() {
            return Err(AppError::BadRequest("conversation not in this project".into()));
        }
        existing
    } else {
        let title = req
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("Task: {}", task.title));
        let new_conv: (Uuid,) = sqlx::query_as(
            "INSERT INTO conversations (project_id, user_id, title, mode)
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(project_id)
        .bind(auth_user.id)
        .bind(title)
        .bind(conv_mode)
        .fetch_one(&state.db)
        .await?;
        new_conv.0
    };

    let prompt = build_task_prompt(&task);

    let attempt: TaskAttempt = sqlx::query_as(
        "WITH inserted AS (
            INSERT INTO task_attempts (task_id, conversation_id, mode, dispatched_by, note, status)
            VALUES ($1, $2, $3, $4, $5, 'pending')
            RETURNING id, task_id, conversation_id, mode, status, dispatched_by, note, created_at, updated_at
         )
         SELECT i.id, i.task_id, i.conversation_id, i.mode, i.status,
                i.dispatched_by, u.display_name AS dispatched_by_name,
                i.note, i.created_at, i.updated_at
         FROM inserted i
         LEFT JOIN users u ON u.id = i.dispatched_by",
    )
    .bind(task_id)
    .bind(conversation_id)
    .bind(mode_trim)
    .bind(auth_user.id)
    .bind(req.note.as_deref())
    .fetch_one(&state.db)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(DispatchResult { attempt, conversation_id, prompt }),
    ))
}

async fn list_task_attempts(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Vec<TaskAttempt>>> {
    verify_access(&state, project_id, auth_user.id).await?;

    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM project_tasks WHERE id = $1 AND project_id = $2",
    )
    .bind(task_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("Task not found".into()));
    }

    let attempts: Vec<TaskAttempt> = sqlx::query_as(
        "SELECT a.id, a.task_id, a.conversation_id, a.mode, a.status,
                a.dispatched_by, u.display_name AS dispatched_by_name,
                a.note, a.created_at, a.updated_at
         FROM task_attempts a
         LEFT JOIN users u ON u.id = a.dispatched_by
         WHERE a.task_id = $1
         ORDER BY a.created_at DESC",
    )
    .bind(task_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(attempts))
}

#[derive(Debug, Serialize, FromRow)]
pub struct TaskComment {
    pub id: Uuid,
    pub task_id: Uuid,
    pub user_id: Uuid,
    pub author_name: Option<String>,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTaskComment {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskComment {
    pub content: String,
}

const COMMENT_SELECT: &str = "SELECT c.id, c.task_id, c.user_id,
        u.display_name AS author_name,
        c.content, c.created_at, c.updated_at
     FROM task_comments c
     LEFT JOIN users u ON u.id = c.user_id";

async fn list_task_comments(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Vec<TaskComment>>> {
    verify_access(&state, project_id, auth_user.id).await?;
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM project_tasks WHERE id = $1 AND project_id = $2",
    )
    .bind(task_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("Task not found".into()));
    }
    let sql = format!("{COMMENT_SELECT} WHERE c.task_id = $1 ORDER BY c.created_at ASC");
    let comments: Vec<TaskComment> = sqlx::query_as(&sql)
        .bind(task_id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(comments))
}

async fn create_task_comment(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<CreateTaskComment>,
) -> AppResult<(StatusCode, Json<TaskComment>)> {
    verify_access(&state, project_id, auth_user.id).await?;
    let trimmed = req.content.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("content is required".into()));
    }
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM project_tasks WHERE id = $1 AND project_id = $2",
    )
    .bind(task_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("Task not found".into()));
    }
    let new_id: (Uuid,) = sqlx::query_as(
        "INSERT INTO task_comments (task_id, user_id, content)
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(task_id)
    .bind(auth_user.id)
    .bind(trimmed)
    .fetch_one(&state.db)
    .await?;
    let sql = format!("{COMMENT_SELECT} WHERE c.id = $1");
    let comment: TaskComment = sqlx::query_as(&sql)
        .bind(new_id.0)
        .fetch_one(&state.db)
        .await?;
    Ok((StatusCode::CREATED, Json(comment)))
}

async fn update_task_comment(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id, comment_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(req): Json<UpdateTaskComment>,
) -> AppResult<Json<TaskComment>> {
    verify_access(&state, project_id, auth_user.id).await?;
    let trimmed = req.content.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("content is required".into()));
    }
    // Only the comment author may edit.
    let updated: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE task_comments SET content = $1, updated_at = NOW()
         WHERE id = $2 AND task_id = $3 AND user_id = $4
         RETURNING id",
    )
    .bind(trimmed)
    .bind(comment_id)
    .bind(task_id)
    .bind(auth_user.id)
    .fetch_optional(&state.db)
    .await?;
    let id = updated.ok_or_else(|| AppError::NotFound("Comment not found or not yours".into()))?.0;
    let sql = format!("{COMMENT_SELECT} WHERE c.id = $1");
    let comment: TaskComment = sqlx::query_as(&sql)
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    Ok(Json(comment))
}

async fn delete_task_comment(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id, comment_id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<StatusCode> {
    verify_access(&state, project_id, auth_user.id).await?;
    let result = sqlx::query(
        "DELETE FROM task_comments
         WHERE id = $1 AND task_id = $2 AND user_id = $3",
    )
    .bind(comment_id)
    .bind(task_id)
    .bind(auth_user.id)
    .execute(&state.db)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Comment not found or not yours".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize, FromRow)]
pub struct TaskAuditEntry {
    pub id: Uuid,
    pub task_id: Uuid,
    pub field: String,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub actor_id: Option<Uuid>,
    pub actor_name: Option<String>,
    pub changed_at: DateTime<Utc>,
}

async fn list_task_audit(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Vec<TaskAuditEntry>>> {
    verify_access(&state, project_id, auth_user.id).await?;
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM project_tasks WHERE id = $1 AND project_id = $2",
    )
    .bind(task_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("Task not found".into()));
    }
    let rows: Vec<TaskAuditEntry> = sqlx::query_as(
        "SELECT a.id, a.task_id, a.field, a.old_value, a.new_value,
                a.actor_id, u.display_name AS actor_name, a.changed_at
         FROM task_audit_log a
         LEFT JOIN users u ON u.id = a.actor_id
         WHERE a.task_id = $1
         ORDER BY a.changed_at DESC",
    )
    .bind(task_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows))
}

async fn list_task_history(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path((project_id, task_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Vec<TaskStatusEvent>>> {
    verify_access(&state, project_id, auth_user.id).await?;

    // Confirm task belongs to this project (avoid leaking other projects' history).
    let exists: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM project_tasks WHERE id = $1 AND project_id = $2",
    )
    .bind(task_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    if exists.is_none() {
        return Err(AppError::NotFound("Task not found".into()));
    }

    let events: Vec<TaskStatusEvent> = sqlx::query_as(
        "SELECT h.id, h.task_id, h.from_status, h.to_status,
                h.changed_by, u.display_name AS changed_by_name,
                h.note, h.changed_at
         FROM task_status_history h
         LEFT JOIN users u ON u.id = h.changed_by
         WHERE h.task_id = $1
         ORDER BY h.changed_at ASC",
    )
    .bind(task_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(events))
}
