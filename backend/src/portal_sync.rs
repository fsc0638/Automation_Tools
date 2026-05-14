//! Orchestrates the KWay portal sync pipeline:
//!   1. spawn `python -m kway_portal meeting-rooms --start … --end …`
//!   2. locate the JSON it just wrote
//!   3. import that JSON via `portal_import::import_scrape`
//!
//! Called from two places: the periodic background scheduler started in
//! main.rs, and the POST /meetings/sync endpoint used by the UI's refresh
//! button. A Mutex prevents the two from running concurrently — both code
//! paths await the same lock so a manual click during an in-flight auto
//! sync is queued, not overlapped.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{Duration, Local};
use serde::Serialize;
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::portal_import::{self, ImportStats};

const FEATURE: &str = "meeting-rooms";

/// Sync window in days, counted from "today" in the server's local timezone.
/// Hard policy: both the manual /meetings/sync endpoint and the periodic
/// scheduler ALWAYS use this exact window. Don't read it from env — making
/// it configurable invites the manual and auto paths drifting apart, which
/// is exactly the bug we promised the operator we'd avoid.
pub const SYNC_DAYS_AHEAD: i64 = 14;

/// Inputs for one sync run. Defaults are filled by `from_env()`.
#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// Path to the kway_portal package root (contains pyproject.toml,
    /// .env, output/, snapshots/, src/). Subprocess runs with this as cwd.
    pub portal_dir: PathBuf,
    /// Python executable to invoke. Defaults to "python".
    pub python_bin: String,
    /// Email of the user that should own imported rows. Falls through to
    /// the resolve_creator_id fallback chain in portal_import.
    pub creator_email: Option<String>,
}

impl SyncOptions {
    pub fn from_env(backend_cwd: &Path) -> Self {
        let portal_dir = std::env::var("KWAY_PORTAL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                // Sensible default when backend runs from the `backend/` dir
                // of the repo: ../kway_portal.
                backend_cwd.join("..").join("kway_portal")
            });
        let python_bin = std::env::var("KWAY_PORTAL_PYTHON").unwrap_or_else(|_| "python".into());
        let creator_email = std::env::var("PORTAL_IMPORT_CREATOR_EMAIL").ok();
        SyncOptions {
            portal_dir,
            python_bin,
            creator_email,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub range_start: String,
    pub range_end: String,
    pub stdout_tail: String,
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub cancelled: usize,
    pub skipped: usize,
    pub elapsed_ms: u128,
}

/// Process-wide gate. Manual + auto sync share this lock so two runs never
/// overlap and trample each other's output directory state.
#[derive(Clone, Default)]
pub struct SyncLock(Arc<Mutex<()>>);

impl SyncLock {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Run one full sync pass: scrape → import. Holds `lock` for the entire
/// duration. If you don't have a SyncLock yet, pass `&SyncLock::new()` —
/// most callers should share a single SyncLock stored on AppState so manual
/// + scheduler block each other.
pub async fn run(pool: &PgPool, lock: &SyncLock, opts: &SyncOptions) -> Result<SyncReport> {
    let _guard = lock.0.lock().await;
    let started = std::time::Instant::now();

    // Window policy: from today (server local time) through today + 14 days.
    // Both auto and manual paths arrive here, so this is the single point of
    // truth for the sync window.
    let today = Local::now().date_naive();
    let end = today + Duration::days(SYNC_DAYS_AHEAD);
    let start_str = today.format("%Y-%m-%d").to_string();
    let end_str = end.format("%Y-%m-%d").to_string();

    tracing::info!("portal_sync: scraping {start_str} → {end_str} ({SYNC_DAYS_AHEAD} day window)");

    let output = Command::new(&opts.python_bin)
        .args([
            "-m",
            "kway_portal",
            "meeting-rooms",
            "--start",
            &start_str,
            "--end",
            &end_str,
        ])
        .current_dir(&opts.portal_dir)
        .env("PYTHONPATH", opts.portal_dir.join("src"))
        .output()
        .await
        .with_context(|| {
            format!(
                "spawning {} -m kway_portal (portal_dir={})",
                opts.python_bin,
                opts.portal_dir.display()
            )
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let combined = format!("{stdout}\n{stderr}");

    if !output.status.success() {
        bail!(
            "scraper exited with status {}: {}",
            output.status,
            tail(&combined, 800)
        );
    }

    let json_path = latest_scrape_json(&opts.portal_dir).context("locating scraper output")?;
    tracing::info!("portal_sync: importing {}", json_path.display());

    let stats: ImportStats = portal_import::import_from_file(
        pool,
        &json_path,
        opts.creator_email.as_deref(),
        false,
    )
    .await
    .context("portal_import failed")?;

    let elapsed_ms = started.elapsed().as_millis();
    tracing::info!(
        "portal_sync: done in {elapsed_ms}ms — inserted={} updated={} unchanged={} cancelled={}",
        stats.inserted,
        stats.updated,
        stats.unchanged,
        stats.cancelled,
    );
    Ok(SyncReport {
        range_start: start_str,
        range_end: end_str,
        stdout_tail: tail(&combined, 400),
        inserted: stats.inserted,
        updated: stats.updated,
        unchanged: stats.unchanged,
        cancelled: stats.cancelled,
        skipped: stats.skipped,
        elapsed_ms,
    })
}

/// Find the most recently modified meeting-rooms JSON the scraper produced.
fn latest_scrape_json(portal_dir: &Path) -> Result<PathBuf> {
    let out_dir = portal_dir.join("output").join(FEATURE);
    let entries = std::fs::read_dir(&out_dir)
        .with_context(|| format!("reading {}", out_dir.display()))?;
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::UNIX_EPOCH);
        if newest.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
            newest = Some((modified, path));
        }
    }
    newest
        .map(|(_, p)| p)
        .ok_or_else(|| anyhow!("no JSON files under {}", out_dir.display()))
}

fn tail(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let cut = s.len() - n;
    let mut start = cut;
    // Trim partial UTF-8 prefix.
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].to_string()
}
