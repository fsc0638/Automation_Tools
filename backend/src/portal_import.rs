//! Shared logic for importing kway_portal meeting-room JSON into the
//! `meetings` table. Used by `bin/import_portal_meetings.rs` (CLI) and by
//! `portal_sync::run` (server-side scheduler + /meetings/sync endpoint).
//!
//! Idempotent: rows are keyed by `external_id` of the shape
//!   "kway-portal:meeting-rooms:<YYYY-MM-DD>:<room_code>:<HHMM>"
//! Re-imports are no-ops when nothing changed; bookings present in the
//! database for the days in the JSON but missing from the JSON itself are
//! marked status='cancelled' (only those still in a live status).

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashSet;
use std::path::Path;
use uuid::Uuid;

pub const SOURCE_SYSTEM: &str = "kway-portal";
pub const FEATURE: &str = "meeting-rooms";
pub const DEFAULT_TIMEZONE: &str = "Asia/Taipei";

#[derive(Debug, Deserialize)]
pub struct ScrapeFile {
    pub range_start: String,
    pub range_end: String,
    #[serde(default)]
    pub days: Vec<DayBlock>,
}

#[derive(Debug, Deserialize)]
pub struct DayBlock {
    pub date: String,
    #[serde(default)]
    pub rooms: Vec<RoomBlock>,
    /// Set by the scraper when that specific day failed to render (e.g.
    /// portal session lost). When present, the importer treats this date
    /// as "no trustworthy data" and won't cancel existing rows for it.
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RoomBlock {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub bookings: Vec<Booking>,
}

#[derive(Debug, Deserialize)]
pub struct Booking {
    pub time_start: String,
    pub time_end: String,
    pub user: String,
    /// Detail fields the scraper pulled from the per-booking preview page
    /// (subject, attendees, etc.). Absent when the detail fetch failed for
    /// that booking — we fall back to a synthesized title in that case.
    #[serde(default)]
    pub details: Option<BookingDetails>,
}

#[derive(Debug, Deserialize, Default)]
pub struct BookingDetails {
    /// 說明 / purpose of the booking. Used as the meeting title when set.
    #[serde(default)]
    pub subject: String,
}

#[derive(Default, Debug, Clone)]
pub struct ImportStats {
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub cancelled: usize,
    pub skipped: usize,
    pub would_upsert: usize,
}

pub enum UpsertOutcome {
    Inserted,
    Updated,
    Unchanged,
}

pub async fn resolve_creator_id(pool: &PgPool, email_arg: Option<&str>) -> Result<Uuid> {
    let env_email = std::env::var("PORTAL_IMPORT_CREATOR_EMAIL").ok();
    if let Some(email) = email_arg.or(env_email.as_deref()) {
        let row: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM users WHERE LOWER(email) = LOWER($1)")
                .bind(email)
                .fetch_optional(pool)
                .await?;
        return row
            .map(|(id,)| id)
            .ok_or_else(|| anyhow!("no user found with email {email}"));
    }
    // Fall back to the oldest user so the importer works on dev boxes
    // before the operator has wired PORTAL_IMPORT_CREATOR_EMAIL.
    let row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, email FROM users ORDER BY created_at ASC LIMIT 1")
            .fetch_optional(pool)
            .await?;
    match row {
        Some((id, email)) => {
            tracing::warn!(
                "no --creator-email / PORTAL_IMPORT_CREATOR_EMAIL set; \
                 defaulting to oldest user {email} ({id})"
            );
            Ok(id)
        }
        None => bail!("no users in DB — create one before importing"),
    }
}

pub fn parse_scrape_file<P: AsRef<Path>>(path: P) -> Result<ScrapeFile> {
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.as_ref().display()))?;
    serde_json::from_str(&raw).context("parsing scrape JSON")
}

pub async fn import_scrape(
    pool: &PgPool,
    scrape: &ScrapeFile,
    creator_id: Uuid,
    dry_run: bool,
) -> Result<ImportStats> {
    let tz: Tz = DEFAULT_TIMEZONE
        .parse()
        .map_err(|e| anyhow!("invalid timezone {DEFAULT_TIMEZONE}: {e}"))?;

    let mut stats = ImportStats::default();
    let mut seen_external_ids: HashSet<String> = HashSet::new();
    // Only dates we successfully scraped end up here; per-day errors are
    // skipped so the cancellation pass doesn't false-positive entire days
    // when the scraper lost session for that page.
    let mut imported_dates: HashSet<String> = HashSet::new();

    let mut total_bookings = 0usize;
    for day in &scrape.days {
        if let Some(err) = day.error.as_deref() {
            tracing::warn!(
                "skip {} import: scraper reported error: {}",
                day.date,
                err
            );
            continue;
        }
        let day_date = NaiveDate::parse_from_str(&day.date, "%Y-%m-%d")
            .with_context(|| format!("bad date {}", day.date))?;
        imported_dates.insert(day.date.clone());

        for room in &day.rooms {
            for booking in &room.bookings {
                let external_id = format!(
                    "{SOURCE_SYSTEM}:{FEATURE}:{}:{}:{}",
                    day.date,
                    room.code,
                    booking.time_start.replace(':', "")
                );
                seen_external_ids.insert(external_id.clone());

                let start_utc = local_to_utc(tz, day_date, &booking.time_start)
                    .with_context(|| format!("start_at for {external_id}"))?;
                let end_utc = local_to_utc(tz, day_date, &booking.time_end)
                    .with_context(|| format!("end_at for {external_id}"))?;
                if end_utc <= start_utc {
                    tracing::warn!(
                        "skip {external_id}: end_at ({}) <= start_at ({})",
                        booking.time_end,
                        booking.time_start
                    );
                    stats.skipped += 1;
                    continue;
                }

                // Prefer the booking's subject (說明) as the meeting title
                // — that's what shows in the detail page's "會議名稱" field
                // and is what people actually want to read. Fall back to a
                // composed "room · user" so the title is never empty for
                // legacy data or detail-fetch failures.
                let subject = booking
                    .details
                    .as_ref()
                    .map(|d| d.subject.trim())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("");
                let title = if subject.is_empty() {
                    format!("{} · {}", room.name, booking.user)
                } else {
                    subject.to_string()
                };
                let location = &room.name;

                if dry_run {
                    stats.would_upsert += 1;
                    continue;
                }

                total_bookings += 1;
                match upsert_meeting(
                    pool,
                    &external_id,
                    creator_id,
                    &booking.user,
                    &title,
                    start_utc,
                    end_utc,
                    location,
                    DEFAULT_TIMEZONE,
                )
                .await?
                {
                    UpsertOutcome::Inserted => stats.inserted += 1,
                    UpsertOutcome::Updated => stats.updated += 1,
                    UpsertOutcome::Unchanged => stats.unchanged += 1,
                }
            }
        }
    }

    // Safety guard: if the scraper produced ZERO bookings across the whole
    // window, almost certainly the portal session dropped or the page
    // failed to render. Treating that as "everyone cancelled" would mass-
    // cancel real bookings. We bail out of cancellation in that case and
    // leave the operator to investigate. A legitimate zero-booking window
    // costs nothing — there's nothing to cancel anyway.
    if total_bookings == 0 && !imported_dates.is_empty() {
        tracing::warn!(
            "scrape produced 0 bookings across {} day(s); refusing to cancel \
             any existing rows. Check the scraper / portal session.",
            imported_dates.len()
        );
        return Ok(stats);
    }

    if !dry_run && !imported_dates.is_empty() {
        let patterns: Vec<String> = imported_dates
            .iter()
            .map(|d| format!("{SOURCE_SYSTEM}:{FEATURE}:{d}:%"))
            .collect();
        let existing: Vec<(String, String)> = sqlx::query_as(
            "SELECT external_id, status FROM meetings
             WHERE external_id ILIKE ANY($1)",
        )
        .bind(&patterns)
        .fetch_all(pool)
        .await?;
        let to_cancel: Vec<String> = existing
            .into_iter()
            .filter(|(ext, status)| {
                !seen_external_ids.contains(ext)
                    && (status == "scheduled" || status == "in_progress")
            })
            .map(|(ext, _)| ext)
            .collect();
        if !to_cancel.is_empty() {
            let n = sqlx::query(
                "UPDATE meetings SET status = 'cancelled', updated_at = NOW()
                 WHERE external_id = ANY($1)",
            )
            .bind(&to_cancel)
            .execute(pool)
            .await?
            .rows_affected();
            stats.cancelled = n as usize;
        }
    }

    Ok(stats)
}

/// Convenience: parse JSON file, resolve creator, run import.
pub async fn import_from_file<P: AsRef<Path>>(
    pool: &PgPool,
    path: P,
    creator_email: Option<&str>,
    dry_run: bool,
) -> Result<ImportStats> {
    let scrape = parse_scrape_file(path)?;
    let creator_id = resolve_creator_id(pool, creator_email).await?;
    import_scrape(pool, &scrape, creator_id, dry_run).await
}

fn local_to_utc(tz: Tz, day: NaiveDate, hhmm: &str) -> Result<DateTime<Utc>> {
    let time = NaiveTime::parse_from_str(hhmm, "%H:%M")
        .with_context(|| format!("invalid time {hhmm}"))?;
    let naive = day.and_time(time);
    let local = tz
        .from_local_datetime(&naive)
        .single()
        .ok_or_else(|| anyhow!("ambiguous local time {naive}"))?;
    Ok(local.with_timezone(&Utc))
}

async fn upsert_meeting(
    pool: &PgPool,
    external_id: &str,
    creator_id: Uuid,
    external_creator_name: &str,
    title: &str,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    location: &str,
    timezone: &str,
) -> Result<UpsertOutcome> {
    let existing: Option<(Uuid, String, DateTime<Utc>, DateTime<Utc>, Option<String>, String, Option<String>)> =
        sqlx::query_as(
            "SELECT id, title, start_at, end_at, location, status, external_creator_name
             FROM meetings WHERE external_id = $1",
        )
        .bind(external_id)
        .fetch_optional(pool)
        .await?;

    let trimmed_name = external_creator_name.trim();
    let creator_name_param: Option<&str> = if trimmed_name.is_empty() { None } else { Some(trimmed_name) };

    if let Some((id, db_title, db_start, db_end, db_location, db_status, db_creator_name)) = existing {
        let same_title = db_title == title;
        let same_window = db_start == start_at && db_end == end_at;
        let same_location = db_location.as_deref() == Some(location);
        let same_status = db_status == "scheduled";
        let same_creator_name = db_creator_name.as_deref() == creator_name_param;
        if same_title && same_window && same_location && same_status && same_creator_name {
            return Ok(UpsertOutcome::Unchanged);
        }
        sqlx::query(
            "UPDATE meetings SET
                title = $1, start_at = $2, end_at = $3, location = $4,
                status = 'scheduled', external_creator_name = $6,
                updated_at = NOW()
             WHERE id = $5",
        )
        .bind(title)
        .bind(start_at)
        .bind(end_at)
        .bind(location)
        .bind(id)
        .bind(creator_name_param)
        .execute(pool)
        .await?;
        return Ok(UpsertOutcome::Updated);
    }

    sqlx::query(
        "INSERT INTO meetings
            (creator_id, title, start_at, end_at, all_day, recurrence,
             timezone, location, status, external_id, external_creator_name,
             notification_note)
         VALUES ($1, $2, $3, $4, false, 'none', $5, $6, 'scheduled', $7, $8,
                 'Imported from KWay portal')",
    )
    .bind(creator_id)
    .bind(title)
    .bind(start_at)
    .bind(end_at)
    .bind(timezone)
    .bind(location)
    .bind(external_id)
    .bind(creator_name_param)
    .execute(pool)
    .await?;
    Ok(UpsertOutcome::Inserted)
}
