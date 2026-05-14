//! Importer for the kway_portal employee-directory feature.
//!
//! Reads the two JSON files written by the Python scraper
//! (employees_<monday>.json + departments_<monday>.json) and upserts into
//! `portal_departments` + `portal_employees`. Each scrape is a full
//! snapshot — we upsert by natural key (code / employee_no) and bump
//! `last_seen_at` on every match, so downstream queries can age out rows
//! whose `last_seen_at` lags behind the latest run without deleting
//! historical records outright.
//!
//! Best-effort link to `users`: when a portal employee's email matches an
//! app user's email (case-insensitive), `portal_employees.user_id` is
//! filled. Keeps meetings + ACL features able to resolve "this attendee
//! is actually that app user".

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct EmployeesFile {
    pub week_starting: String,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub employees: Vec<Employee>,
}

#[derive(Debug, Deserialize)]
pub struct DepartmentsFile {
    pub week_starting: String,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub departments: Vec<Department>,
}

#[derive(Debug, Deserialize)]
pub struct Employee {
    pub employee_no: String,
    pub name: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub dept_code: String,
    #[serde(default)]
    pub dept_name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct Department {
    pub code: String,
    pub name: String,
}

#[derive(Default, Debug, Clone)]
pub struct DirectoryImportStats {
    pub departments_upserted: usize,
    pub employees_upserted: usize,
    pub employees_linked_to_user: usize,
    pub skipped: usize,
}

pub fn parse_employees<P: AsRef<Path>>(path: P) -> Result<EmployeesFile> {
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.as_ref().display()))?;
    serde_json::from_str(&raw).context("parsing employees JSON")
}

pub fn parse_departments<P: AsRef<Path>>(path: P) -> Result<DepartmentsFile> {
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading {}", path.as_ref().display()))?;
    serde_json::from_str(&raw).context("parsing departments JSON")
}

/// Locate the latest pair of (employees_*.json, departments_*.json) under
/// the scraper's output directory. Returns paths to the most recent week.
pub fn latest_pair(output_root: &Path) -> Result<(PathBuf, PathBuf)> {
    let dir = output_root.join("employee-directory");
    let mut newest_emp: Option<(std::time::SystemTime, PathBuf, String)> = None;
    let mut newest_dept: Option<(std::time::SystemTime, PathBuf, String)> = None;

    let entries = std::fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let modified = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::UNIX_EPOCH);
        if name.starts_with("employees_") && name.ends_with(".json") {
            let week = name
                .trim_start_matches("employees_")
                .trim_end_matches(".json")
                .to_string();
            if newest_emp.as_ref().map(|(t, _, _)| modified > *t).unwrap_or(true) {
                newest_emp = Some((modified, path.clone(), week));
            }
        } else if name.starts_with("departments_") && name.ends_with(".json") {
            let week = name
                .trim_start_matches("departments_")
                .trim_end_matches(".json")
                .to_string();
            if newest_dept.as_ref().map(|(t, _, _)| modified > *t).unwrap_or(true) {
                newest_dept = Some((modified, path.clone(), week));
            }
        }
    }

    let (_, emp_path, emp_week) = newest_emp
        .ok_or_else(|| anyhow!("no employees_*.json under {}", dir.display()))?;
    let (_, dept_path, dept_week) = newest_dept
        .ok_or_else(|| anyhow!("no departments_*.json under {}", dir.display()))?;
    if emp_week != dept_week {
        bail!(
            "employees week {} != departments week {}; scraper ran inconsistently",
            emp_week,
            dept_week
        );
    }
    Ok((emp_path, dept_path))
}

pub async fn import_directory(
    pool: &PgPool,
    employees_path: &Path,
    departments_path: &Path,
) -> Result<DirectoryImportStats> {
    let depts = parse_departments(departments_path)?;
    let emps = parse_employees(employees_path)?;

    // Refuse to nuke an existing directory snapshot when the scraper came
    // back with zero rows (almost always a session/scrape failure). The
    // user explicitly asked for a failure log, but defending the DB is
    // even more important — keep the previous good data.
    if emps.employees.is_empty() && depts.departments.is_empty() {
        bail!("scraper produced 0 employees and 0 departments — refusing to import");
    }

    let mut stats = DirectoryImportStats::default();

    for d in &depts.departments {
        if d.code.trim().is_empty() {
            stats.skipped += 1;
            continue;
        }
        sqlx::query(
            "INSERT INTO portal_departments (code, name, last_seen_at)
             VALUES ($1, $2, NOW())
             ON CONFLICT (code)
             DO UPDATE SET
                name = EXCLUDED.name,
                last_seen_at = NOW(),
                updated_at = NOW()",
        )
        .bind(&d.code)
        .bind(&d.name)
        .execute(pool)
        .await?;
        stats.departments_upserted += 1;
    }

    for e in &emps.employees {
        if e.employee_no.trim().is_empty() {
            stats.skipped += 1;
            continue;
        }
        let email = e.email.trim();
        let email_param: Option<&str> = if email.is_empty() { None } else { Some(email) };

        // Best-effort user match: if an app user shares this email, link
        // them. Multiple-email collisions are unlikely (we trust users.email
        // as unique-ish); first match wins.
        let user_id: Option<Uuid> = if let Some(em) = email_param {
            sqlx::query_scalar("SELECT id FROM users WHERE LOWER(email) = LOWER($1) LIMIT 1")
                .bind(em)
                .fetch_optional(pool)
                .await?
        } else {
            None
        };
        if user_id.is_some() {
            stats.employees_linked_to_user += 1;
        }

        let dept_code: Option<&str> = if e.dept_code.trim().is_empty() {
            None
        } else {
            Some(e.dept_code.trim())
        };
        let title: Option<&str> = if e.title.trim().is_empty() {
            None
        } else {
            Some(e.title.trim())
        };

        sqlx::query(
            "INSERT INTO portal_employees
                (employee_no, name, extensions, email, title, dept_code,
                 user_id, last_seen_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
             ON CONFLICT (employee_no)
             DO UPDATE SET
                name = EXCLUDED.name,
                extensions = EXCLUDED.extensions,
                email = EXCLUDED.email,
                title = EXCLUDED.title,
                dept_code = EXCLUDED.dept_code,
                user_id = COALESCE(EXCLUDED.user_id, portal_employees.user_id),
                last_seen_at = NOW(),
                updated_at = NOW()",
        )
        .bind(e.employee_no.trim())
        .bind(e.name.trim())
        .bind(&e.extensions)
        .bind(email_param)
        .bind(title)
        .bind(dept_code)
        .bind(user_id)
        .execute(pool)
        .await?;
        stats.employees_upserted += 1;
    }

    Ok(stats)
}
