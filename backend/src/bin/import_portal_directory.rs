//! CLI for one-shot import of kway_portal employee-directory JSONs.
//!
//! Pairs with `kway_portal/output/employee-directory/employees_*.json` +
//! `departments_*.json`. Use this when you want to backfill or manually
//! re-run the weekly importer outside the Monday 08:00 scheduler.
//!
//! Usage:
//!   cargo run --bin import_portal_directory -- \
//!       [--employees <path>] [--departments <path>]
//!   (Defaults: latest pair under <repo>/kway_portal/output/employee-directory/)

use anyhow::{Context, Result, anyhow, bail};
use kway_dev_backend::portal_directory_import;
use sqlx::PgPool;
use std::path::PathBuf;

struct Args {
    employees: Option<PathBuf>,
    departments: Option<PathBuf>,
    portal_dir: PathBuf,
}

fn parse_args() -> Result<Args> {
    let mut employees: Option<PathBuf> = None;
    let mut departments: Option<PathBuf> = None;
    let mut portal_dir = PathBuf::from(
        std::env::var("KWAY_PORTAL_DIR").unwrap_or_else(|_| "../kway_portal".into()),
    );

    let mut iter = std::env::args().skip(1);
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--employees" => employees = Some(PathBuf::from(iter.next().context("--employees")?)),
            "--departments" => {
                departments = Some(PathBuf::from(iter.next().context("--departments")?))
            }
            "--portal-dir" => portal_dir = PathBuf::from(iter.next().context("--portal-dir")?),
            "-h" | "--help" => {
                eprintln!(
                    "Usage: import_portal_directory \\\n  \
                     [--employees <path>] [--departments <path>] \\\n  \
                     [--portal-dir <kway_portal root>]\n\n\
                     Defaults to the most recent pair under \
                     <portal_dir>/output/employee-directory/."
                );
                std::process::exit(0);
            }
            other => bail!("unknown argument: {other}"),
        }
    }
    Ok(Args {
        employees,
        departments,
        portal_dir,
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = parse_args()?;
    dotenvy::dotenv().ok();
    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL is required")?;
    let pool = PgPool::connect(&db_url).await.context("connecting to DB")?;

    let (emp, dept) = match (args.employees, args.departments) {
        (Some(e), Some(d)) => (e, d),
        (None, None) => portal_directory_import::latest_pair(
            &args.portal_dir.join("output"),
        )
        .context("auto-locating JSON pair")?,
        _ => return Err(anyhow!("specify BOTH --employees and --departments, or NEITHER")),
    };
    eprintln!(
        "[plan] employees={} departments={}",
        emp.display(),
        dept.display()
    );

    let stats = portal_directory_import::import_directory(&pool, &emp, &dept).await?;
    eprintln!(
        "[done] departments={} employees={} linked_users={} skipped={}",
        stats.departments_upserted,
        stats.employees_upserted,
        stats.employees_linked_to_user,
        stats.skipped,
    );
    Ok(())
}
