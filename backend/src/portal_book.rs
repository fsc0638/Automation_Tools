//! Write-side companion to `portal_sync` — drives the KWay portal's
//! 預約 / 取消 forms via the `kway_portal meeting-book` Playwright feature.
//!
//! The Rust side stays thin on purpose: shape the command-line args from
//! a Meeting row, spawn the subprocess, parse the JSON it leaves on disk,
//! and translate `{success, error}` back into something the meetings API
//! can stamp onto the DB.
//!
//! Why one feature with --op rather than three separate scripts: each
//! invocation pays the Playwright launch cost (~3s on a warm box, more on
//! cold), and the three forms share login + room-picker logic. Keeping it
//! in one feature lets us amortise that once if we ever batch.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Asia::Taipei;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::process::Command;

use crate::portal_sync::SyncOptions;

/// Mirrors the JSON the `meeting-book` Python feature writes. `error` is
/// present iff `success == false` (but absent fields decode as `None` so
/// we tolerate the optimistic "no error tokens detected" path too).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BookResult {
    pub op: String,
    pub success: bool,
    pub room_code: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub submitted_url: Option<String>,
    #[serde(default)]
    pub snapshot_html: Option<String>,
}

/// One booking attempt. Fields mirror the Python CLI's flags. Building
/// the struct in code (rather than threading 10 separate args) keeps the
/// call sites — create_meeting, delete_meeting, the backfill binary —
/// readable.
#[derive(Debug, Clone)]
pub struct BookRequest {
    pub op: BookOp,
    pub room_code: String,
    pub room_name: String,
    /// Single-day booking: this is the date. Multi/cancel: leave empty.
    pub date: Option<String>,
    /// Multi/cancel: range bounds (YYYY-MM-DD). Single-day: leave empty.
    pub date_start: Option<String>,
    pub date_end: Option<String>,
    pub period_weeks: i32,
    pub time_start: String, // HH:MM
    pub time_end: String,
    pub subject: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookOp {
    Book,
    BookMulti,
    Cancel,
}

impl BookOp {
    pub fn as_arg(&self) -> &'static str {
        match self {
            BookOp::Book => "book",
            BookOp::BookMulti => "book-multi",
            BookOp::Cancel => "cancel",
        }
    }
}

/// Derive the (room_code, room_name) pair from a meetings.location string.
/// Portal scrape stores names like "5號會議室(8人)" and the room_code is
/// recoverable by looking it up against `portal_rooms`-equivalent data;
/// for now we rely on portal_employees-style enrichment elsewhere. When
/// the caller already knows the code (we stash it in `external_id`'s
/// prefix), the helper falls through. Returns None when we have neither
/// a code nor an obvious name to feed the dropdown.
pub fn split_location(location: Option<&str>) -> Option<(String, String)> {
    let name = location?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    // Strip an optional "Room CODE / display" prefix we sometimes use in
    // app-created meetings. The portal's <select> labels match `name`
    // directly so passing the cleaned label is enough.
    let cleaned = name
        .split(" / ")
        .next()
        .unwrap_or(&name)
        .trim()
        .to_string();
    // We don't have an authoritative code here — Python feature falls back
    // to label match, which is fine for now.
    Some((String::new(), cleaned))
}

/// Convert a UTC datetime to "YYYY-MM-DD" in Asia/Taipei (the portal's
/// local clock). The portal's 預約日期 field is interpreted as Taipei
/// time, so this matters during the cross-midnight window.
pub fn local_date_str(dt: DateTime<Utc>) -> String {
    Taipei.from_utc_datetime(&dt.naive_utc()).format("%Y-%m-%d").to_string()
}

pub fn local_time_str(dt: DateTime<Utc>) -> String {
    Taipei.from_utc_datetime(&dt.naive_utc()).format("%H:%M").to_string()
}

/// Spawn `python -m kway_portal meeting-book …` with the right flags and
/// return the parsed result. Failures during subprocess launch / exit are
/// distinguished from `success: false` returned by the portal — the
/// caller usually treats both the same (keep meeting as draft), but tests
/// and logs benefit from the distinction.
pub async fn run(opts: &SyncOptions, req: &BookRequest) -> Result<BookResult> {
    let mut cmd = Command::new(&opts.python_bin);
    cmd.args(["-m", "kway_portal", "meeting-book", "--op", req.op.as_arg()]);
    cmd.args(["--room-code", &req.room_code]);
    if !req.room_name.is_empty() {
        cmd.args(["--room-name", &req.room_name]);
    }
    if let Some(d) = &req.date {
        cmd.args(["--date", d]);
    }
    if let Some(d) = &req.date_start {
        cmd.args(["--date-start", d]);
    }
    if let Some(d) = &req.date_end {
        cmd.args(["--date-end", d]);
    }
    cmd.args(["--period-weeks", &req.period_weeks.to_string()]);
    cmd.args(["--time-start", &req.time_start]);
    cmd.args(["--time-end", &req.time_end]);
    if !req.subject.is_empty() {
        cmd.args(["--subject", &req.subject]);
    }

    cmd.current_dir(&opts.portal_dir)
        .env("PYTHONPATH", opts.portal_dir.join("src"));

    let output = cmd.output().await.with_context(|| {
        format!(
            "spawning {} -m kway_portal meeting-book (portal_dir={})",
            opts.python_bin,
            opts.portal_dir.display()
        )
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        bail!(
            "meeting-book exited with status {}: {}",
            output.status,
            tail(&format!("{stdout}\n{stderr}"), 800)
        );
    }
    // CLI prints "[ok] wrote <path>" for us — grab the path, read JSON.
    let json_path = latest_book_json(&opts.portal_dir).context("locating meeting-book output")?;
    let raw = std::fs::read_to_string(&json_path)
        .with_context(|| format!("reading {}", json_path.display()))?;
    let parsed: BookResult = serde_json::from_str(&raw)
        .with_context(|| format!("parsing {}", json_path.display()))?;
    Ok(parsed)
}

/// Find the most recent JSON under output/meeting-book/. The Python CLI
/// writes one file per (op, date, room) so a manual rerun keeps history;
/// "most recent by mtime" picks up the file we just wrote.
fn latest_book_json(portal_dir: &Path) -> Result<std::path::PathBuf> {
    let out_dir = portal_dir.join("output").join("meeting-book");
    let entries = std::fs::read_dir(&out_dir)
        .with_context(|| format!("reading {}", out_dir.display()))?;
    let mut newest: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
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
        .ok_or_else(|| anyhow::anyhow!("no JSON files under {}", out_dir.display()))
}

fn tail(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let cut = s.len() - n;
    let mut start = cut;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].to_string()
}
