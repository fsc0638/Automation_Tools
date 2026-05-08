use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_INDEX_FILE_BYTES: u64 = 1_000_000;
const MAX_INDEX_FILES: usize = 2_000;
const CHUNK_CHARS: usize = 4_000;
const MAX_RELEVANT_CHUNKS: i64 = 8;

const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".next",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "coverage",
];

const TEXT_EXTENSIONS: &[&str] = &[
    "rs",
    "ts",
    "tsx",
    "js",
    "jsx",
    "json",
    "md",
    "toml",
    "yaml",
    "yml",
    "sql",
    "html",
    "css",
    "scss",
    "py",
    "go",
    "java",
    "kt",
    "swift",
    "php",
    "rb",
    "cs",
    "cpp",
    "c",
    "h",
    "hpp",
    "xml",
    "env",
    "example",
    "txt",
    "Dockerfile",
];

pub async fn rebuild_project_index(db: &PgPool, project_id: Uuid, root: &str) -> Result<usize> {
    let root_path = Path::new(root);
    if !root_path.exists() {
        return Err(anyhow!("project root does not exist: {}", root));
    }

    sqlx::query("DELETE FROM project_file_chunks WHERE project_id = $1")
        .bind(project_id)
        .execute(db)
        .await?;
    sqlx::query("DELETE FROM project_files WHERE project_id = $1")
        .bind(project_id)
        .execute(db)
        .await?;

    let mut files = Vec::new();
    collect_indexable_files(root_path, root_path, &mut files)?;
    files.truncate(MAX_INDEX_FILES);

    let mut indexed = 0usize;
    for file in files {
        let metadata = match std::fs::metadata(&file) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.len() > MAX_INDEX_FILE_BYTES {
            continue;
        }
        let content = match std::fs::read_to_string(&file) {
            Ok(c) if !c.trim().is_empty() => c,
            _ => continue,
        };
        let rel = file
            .strip_prefix(root_path)
            .unwrap_or(&file)
            .to_string_lossy()
            .replace('\\', "/");
        let hash = format!("{:x}", Sha256::digest(content.as_bytes()));
        let language = detect_language(&rel);

        let file_id: Uuid = sqlx::query_scalar(
            "INSERT INTO project_files (project_id, path, language, size_bytes, content_hash, indexed_at)
             VALUES ($1, $2, $3, $4, $5, NOW())
             RETURNING id",
        )
        .bind(project_id)
        .bind(&rel)
        .bind(language)
        .bind(metadata.len() as i64)
        .bind(&hash)
        .fetch_one(db)
        .await?;

        for (chunk_index, chunk) in split_chunks(&content).into_iter().enumerate() {
            sqlx::query(
                "INSERT INTO project_file_chunks (project_file_id, project_id, path, chunk_index, content, indexed_at)
                 VALUES ($1, $2, $3, $4, $5, NOW())",
            )
            .bind(file_id)
            .bind(project_id)
            .bind(&rel)
            .bind(chunk_index as i32)
            .bind(chunk)
            .execute(db)
            .await?;
        }
        indexed += 1;
    }

    Ok(indexed)
}

pub async fn relevant_file_context(
    db: &PgPool,
    project_id: Uuid,
    query: &str,
) -> Result<Option<String>> {
    let terms = query_terms(query);
    if terms.is_empty() {
        return Ok(None);
    }

    let chunks: Vec<(String, i32, String)> = sqlx::query_as(
        "SELECT path, chunk_index, content
         FROM project_file_chunks
         WHERE project_id = $1
         ORDER BY indexed_at DESC
         LIMIT 800",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    let mut scored = chunks
        .into_iter()
        .filter_map(|(path, idx, content)| {
            let score = score_chunk(&path, &content, &terms);
            (score > 0).then_some((score, path, idx, content))
        })
        .collect::<Vec<_>>();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    scored.truncate(MAX_RELEVANT_CHUNKS as usize);

    if scored.is_empty() {
        return Ok(None);
    }

    let mut out = String::from(
        "Relevant indexed project file excerpts selected for the current user question. Use these as concrete evidence; cite paths when making recommendations.\n",
    );
    for (_, path, idx, content) in scored {
        let snippet: String = content.chars().take(2_000).collect();
        out.push_str(&format!(
            "\n--- INDEXED FILE: {} [chunk {}] ---\n{}\n",
            path, idx, snippet
        ));
    }

    Ok(Some(out))
}

fn collect_indexable_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if out.len() >= MAX_INDEX_FILES {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .collect::<Vec<_>>();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        if out.len() >= MAX_INDEX_FILES {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            if IGNORED_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            collect_indexable_files(root, &path, out)?;
        } else if file_type.is_file() && is_text_file(&path) && path.starts_with(root) {
            out.push(path);
        }
    }
    Ok(())
}

fn is_text_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if TEXT_EXTENSIONS.contains(&name) {
        return true;
    }
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| TEXT_EXTENSIONS.contains(&ext))
        .unwrap_or(false)
}

fn detect_language(path: &str) -> Option<String> {
    Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .or_else(|| path.ends_with("Dockerfile").then_some("dockerfile".into()))
}

fn split_chunks(content: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut buf = String::new();
    for ch in content.chars() {
        buf.push(ch);
        if buf.chars().count() >= CHUNK_CHARS {
            chunks.push(std::mem::take(&mut buf));
        }
    }
    if !buf.is_empty() {
        chunks.push(buf);
    }
    chunks
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && !is_cjk(c))
        .map(|s| s.trim().to_lowercase())
        .filter(|s| s.chars().count() >= 2)
        .take(24)
        .collect()
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

fn score_chunk(path: &str, content: &str, terms: &[String]) -> i32 {
    let path_l = path.to_lowercase();
    let content_l = content.to_lowercase();
    let mut score = 0;
    for term in terms {
        if path_l.contains(term) {
            score += 8;
        }
        let count = content_l.matches(term).take(8).count() as i32;
        score += count;
    }
    score
}
