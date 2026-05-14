//! CLI for one-shot imports of kway_portal meeting-room JSON.
//!
//! All actual work lives in `kway_dev_backend::portal_import`; this file is
//! just argument parsing + a stat print at the end so the same logic can be
//! reused by the server-side scheduler.
//!
//! Usage:
//!   cargo run --bin import_portal_meetings -- \
//!     --json kway_portal/output/meeting-rooms/<file>.json \
//!     [--creator-email user@kway.com.tw] [--dry-run]

use anyhow::{anyhow, bail, Context, Result};
use kway_dev_backend::portal_import;
use sqlx::PgPool;
use std::path::PathBuf;

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

    eprintln!("[plan] reading {}", args.json_path.display());
    let pool = PgPool::connect(&db_url).await.context("connecting to DB")?;

    let stats = portal_import::import_from_file(
        &pool,
        &args.json_path,
        args.creator_email.as_deref(),
        args.dry_run,
    )
    .await?;

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
