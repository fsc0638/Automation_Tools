//! One-shot CLI: push every "未來、已 scheduled、尚未 portal-booked" meeting
//! up to the KWay portal. Used once after enabling portal-book to backfill
//! the historical meetings the operator created before the integration
//! existed.
//!
//! Idempotent: rows that already have `portal_booked_at` set are skipped.
//! Failures stamp `portal_book_error` and flip the row back to 'draft' so
//! the operator can retry.
//!
//! Usage:
//!   cargo run --release --bin backfill_portal_bookings
//!
//! Set KWAY_PORTAL_DIR, KWAY_PORTAL_PYTHON, DATABASE_URL as usual. The
//! tool walks the meetings table sequentially — Playwright is single-
//! threaded per profile so parallelising won't help.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use kway_dev_backend::{
    portal_book::{self, BookOp, BookRequest},
    portal_sync::SyncOptions,
};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow)]
struct PendingRow {
    id: Uuid,
    title: String,
    location: Option<String>,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    recurrence: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let pool = PgPool::connect(&db_url).await.context("connecting to DB")?;
    let backend_cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let opts = SyncOptions::from_env(&backend_cwd);

    let rows: Vec<PendingRow> = sqlx::query_as(
        "SELECT id, title, location, start_at, end_at, recurrence
         FROM meetings
         WHERE status = 'scheduled'
           AND portal_booked_at IS NULL
           AND start_at > NOW()
           AND location IS NOT NULL
           AND location <> ''
         ORDER BY start_at",
    )
    .fetch_all(&pool)
    .await?;

    eprintln!("[plan] {} meetings to backfill", rows.len());

    let mut ok = 0usize;
    let mut failed = 0usize;
    for row in rows {
        let location = row.location.as_deref().unwrap_or("");
        let Some((room_code, room_name)) = portal_book::split_location(Some(location)) else {
            eprintln!("[skip] {} ({}): no recognisable room name", row.id, row.title);
            continue;
        };
        let op = match row.recurrence.as_str() {
            "weekly" => BookOp::BookMulti,
            _ => BookOp::Book,
        };
        let date = portal_book::local_date_str(row.start_at);
        let req = BookRequest {
            op,
            room_code,
            room_name,
            date: if op == BookOp::Book { Some(date.clone()) } else { None },
            date_start: if op == BookOp::BookMulti { Some(date.clone()) } else { None },
            date_end: if op == BookOp::BookMulti { Some(date.clone()) } else { None },
            period_weeks: 1,
            time_start: portal_book::local_time_str(row.start_at),
            time_end: portal_book::local_time_str(row.end_at),
            subject: row.title.clone(),
        };

        eprintln!(
            "[try] {} {} {} {}–{}",
            row.id,
            req.op.as_arg(),
            req.room_name,
            req.time_start,
            req.time_end
        );
        match portal_book::run(&opts, &req).await {
            Ok(r) if r.success => {
                sqlx::query(
                    "UPDATE meetings SET portal_booked_at = NOW(),
                        portal_book_error = NULL, updated_at = NOW()
                     WHERE id = $1",
                )
                .bind(row.id)
                .execute(&pool)
                .await?;
                ok += 1;
                eprintln!("[ok]  {}", row.id);
            }
            Ok(r) => {
                let err = r.error.unwrap_or_else(|| "success=false without error".into());
                sqlx::query(
                    "UPDATE meetings SET portal_book_error = $2, updated_at = NOW()
                     WHERE id = $1",
                )
                .bind(row.id)
                .bind(&err)
                .execute(&pool)
                .await?;
                failed += 1;
                eprintln!("[err] {}: {}", row.id, err);
            }
            Err(e) => {
                let err = format!("subprocess: {e}");
                sqlx::query(
                    "UPDATE meetings SET portal_book_error = $2, updated_at = NOW()
                     WHERE id = $1",
                )
                .bind(row.id)
                .bind(&err)
                .execute(&pool)
                .await?;
                failed += 1;
                eprintln!("[err] {}: {}", row.id, err);
            }
        }
    }

    eprintln!("[done] ok={ok} failed={failed}");
    Ok(())
}
