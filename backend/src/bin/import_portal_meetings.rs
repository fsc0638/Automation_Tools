//! Import KWay portal meeting-room scraper output into the `meetings` table.
//!
//! Pairs with `kway_portal/` (the Python Playwright client). The scraper
//! writes JSON of shape `{days: [{date, rooms: [{code, name, bookings: [...]}]}]}`;
//! we walk that, upserting one meeting row per booking, keyed by
//! `external_id = "kway-portal:meeting-rooms:<date>:<room_code>:<HHMM>"`.
//!
//! Usage:
//!   cargo run --bin import_portal_meetings -- \
//!     --json kway_portal/output/meeting-rooms/<file>.json \
//!     [--creator-email user@kway.com.tw] [--dry-run]
//!
//! Re-running with the same JSON is idempotent: matching external_ids
//! UPDATE in place; bookings that previously existed but are missing from
//! this run's date range are marked status='cancelled' (only those still in
//! a live status — 'scheduled' or 'in_progress').

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashSet;
use std::path::PathBuf;
use uuid::Uuid;

const SOURCE_SYSTEM: &str = "kway-portal";
const FEATURE: &str = "meeting-rooms";
const DEFAULT_TIMEZONE: &str = "Asia/Taipei";

#[derive(Debug, Deserialize)]
struct ScrapeFile {
    range_start: String,
    range_end: String,
    days: Vec<DayBlock>,
}

#[derive(Debug, Deserialize)]
struct DayBlock {
    date: String, // YYYY-MM-DD
    #[serde(default)]
    rooms: Vec<RoomBlock>,
}

#[derive(Debug, Deserialize)]
struct RoomBlock {
    code: String,
    name: String,
    #[serde(default)]
    bookings: Vec<Booking>,
}

#[derive(Debug, Deserialize)]
struct Booking {
    time_start: String, // "HH:MM"
    time_end: String,
    user: String,
}

struct Args {
    json_path: PathBuf,
    creator_email: Option<String>,
    dry_run: bool,
}

fn parse_args() -> Result<Args> {
    let mut json_path: Option<PathBuf> = None;
    let mut creator_email: Option<String> = None;
    let mut dry_run = false;

    let mut iter = std::env::args().skip(1);
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--json" => {
                json_path = Some(PathBuf::from(
                    iter.next().context("--json requires a path argument")?,
                ));
            }
            "--creator-email" => {
                creator_email = Some(iter.next().context("--creator-email requires a value")?);
            }
            "--dry-run" => dry_run = true,
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => bail!("unknown argument: {other}"),
        }
    }

    let json_path = json_path.ok_or_else(|| {
        anyhow!("--json <path> is required (try --help for usage)")
    })?;
    Ok(Args {
        json_path,
        creator_email,
        dry_run,
    })
}

fn print_usage() {
    eprintln!(
        "Usage: import_portal_meetings --json <path> [--creator-email <email>] [--dry-run]\n\n\
        Reads a kway_portal meeting-rooms JSON file and upserts rows into the\n\
        meetings table. Re-runs are idempotent (matched by external_id).\n\n\
        --json <path>          Required. Path to the scraper output JSON.\n\
        --creator-email <e>    User to own the imported meetings.\n\
                               Defaults to env PORTAL_IMPORT_CREATOR_EMAIL,\n\
                               then to the oldest user in the DB.\n\
        --dry-run              Parse + plan only. No DB writes.\n"
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args()?;
    dotenvy::dotenv().ok();
    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;

    let raw = std::fs::read_to_string(&args.json_path)
        .with_context(|| format!("reading {}", args.json_path.display()))?;
    let scrape: ScrapeFile = serde_json::from_str(&raw).context("parsing scrape JSON")?;
    eprintln!(
        "[plan] {} → {}, {} day(s)",
        scrape.range_start,
        scrape.range_end,
        scrape.days.len()
    );

    let pool = PgPool::connect(&db_url).await.context("connecting to DB")?;

    let creator_id =
        resolve_creator_id(&pool, args.creator_email.as_deref()).await?;
    eprintln!("[ok] creator_id resolved = {}", creator_id);

    let tz: Tz = DEFAULT_TIMEZONE
        .parse()
        .map_err(|e| anyhow!("invalid timezone {DEFAULT_TIMEZONE}: {e}"))?;

    let mut stats = ImportStats::default();
    let mut seen_external_ids: HashSet<String> = HashSet::new();
    let mut imported_dates: HashSet<String> = HashSet::new();

    for day in &scrape.days {
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
                    eprintln!(
                        "[skip] {external_id}: end_at ({}) <= start_at ({})",
                        booking.time_end, booking.time_start
                    );
                    stats.skipped += 1;
                    continue;
                }

                let title = format!("{} · {}", room.name, booking.user);
                let location = room.name.clone();

                if args.dry_run {
                    stats.would_upsert += 1;
                    continue;
                }

                let outcome = upsert_meeting(
                    &pool,
                    &external_id,
                    creator_id,
                    &title,
                    start_utc,
                    end_utc,
                    &location,
                    DEFAULT_TIMEZONE,
                )
                .await?;
                match outcome {
                    UpsertOutcome::Inserted => stats.inserted += 1,
                    UpsertOutcome::Updated => stats.updated += 1,
                    UpsertOutcome::Unchanged => stats.unchanged += 1,
                }
            }
        }
    }

    // Bookings that previously had external_ids for the imported dates but
    // are missing now → mark cancelled (only if still in a live status).
    if !args.dry_run && !imported_dates.is_empty() {
        let prefix_patterns: Vec<String> = imported_dates
            .iter()
            .map(|d| format!("{SOURCE_SYSTEM}:{FEATURE}:{d}:%"))
            .collect();
        let existing: Vec<(String, String)> = sqlx::query_as(
            "SELECT external_id, status FROM meetings
             WHERE external_id ILIKE ANY($1)",
        )
        .bind(&prefix_patterns)
        .fetch_all(&pool)
        .await?;
        let mut to_cancel: Vec<String> = Vec::new();
        for (ext, status) in existing {
            if !seen_external_ids.contains(&ext)
                && (status == "scheduled" || status == "in_progress")
            {
                to_cancel.push(ext);
            }
        }
        if !to_cancel.is_empty() {
            let n = sqlx::query(
                "UPDATE meetings SET status = 'cancelled', updated_at = NOW()
                 WHERE external_id = ANY($1)",
            )
            .bind(&to_cancel)
            .execute(&pool)
            .await?
            .rows_affected();
            stats.cancelled = n as usize;
        }
    }

    eprintln!(
        "[done] inserted={} updated={} unchanged={} cancelled={} skipped={} would_upsert={}",
        stats.inserted,
        stats.updated,
        stats.unchanged,
        stats.cancelled,
        stats.skipped,
        stats.would_upsert,
    );
    Ok(())
}

#[derive(Default)]
struct ImportStats {
    inserted: usize,
    updated: usize,
    unchanged: usize,
    cancelled: usize,
    skipped: usize,
    would_upsert: usize,
}

enum UpsertOutcome {
    Inserted,
    Updated,
    Unchanged,
}

async fn resolve_creator_id(pool: &PgPool, email_arg: Option<&str>) -> Result<Uuid> {
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
    // Fall back to the oldest user — keeps imports working in dev where one
    // user typically owns everything.
    let row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, email FROM users ORDER BY created_at ASC LIMIT 1")
            .fetch_optional(pool)
            .await?;
    match row {
        Some((id, email)) => {
            eprintln!(
                "[warn] no --creator-email or PORTAL_IMPORT_CREATOR_EMAIL set; \
                 defaulting to oldest user {email} ({id})"
            );
            Ok(id)
        }
        None => bail!("no users in DB — create one before importing"),
    }
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
    title: &str,
    start_at: DateTime<Utc>,
    end_at: DateTime<Utc>,
    location: &str,
    timezone: &str,
) -> Result<UpsertOutcome> {
    // SELECT the live row first so we can decide insert vs update vs no-op
    // without paying for an INSERT every run.
    let existing: Option<(Uuid, String, DateTime<Utc>, DateTime<Utc>, Option<String>, String)> =
        sqlx::query_as(
            "SELECT id, title, start_at, end_at, location, status
             FROM meetings WHERE external_id = $1",
        )
        .bind(external_id)
        .fetch_optional(pool)
        .await?;

    if let Some((id, db_title, db_start, db_end, db_location, db_status)) = existing {
        // Re-import always brings status back to 'scheduled' — the portal is
        // the source of truth for "this booking still exists".
        let same_title = db_title == title;
        let same_window = db_start == start_at && db_end == end_at;
        let same_location = db_location.as_deref() == Some(location);
        let same_status = db_status == "scheduled";
        if same_title && same_window && same_location && same_status {
            return Ok(UpsertOutcome::Unchanged);
        }
        sqlx::query(
            "UPDATE meetings SET
                title = $1, start_at = $2, end_at = $3, location = $4,
                status = 'scheduled', updated_at = NOW()
             WHERE id = $5",
        )
        .bind(title)
        .bind(start_at)
        .bind(end_at)
        .bind(location)
        .bind(id)
        .execute(pool)
        .await?;
        return Ok(UpsertOutcome::Updated);
    }

    sqlx::query(
        "INSERT INTO meetings
            (creator_id, title, start_at, end_at, all_day, recurrence,
             timezone, location, status, external_id, notification_note)
         VALUES ($1, $2, $3, $4, false, 'none', $5, $6, 'scheduled', $7,
                 'Imported from KWay portal')",
    )
    .bind(creator_id)
    .bind(title)
    .bind(start_at)
    .bind(end_at)
    .bind(timezone)
    .bind(location)
    .bind(external_id)
    .execute(pool)
    .await?;
    Ok(UpsertOutcome::Inserted)
}
