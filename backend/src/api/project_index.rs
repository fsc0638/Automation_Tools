use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_INDEX_FILE_BYTES: u64 = 1_000_000;
const MAX_INDEX_FILES: usize = 2_000;
const CHUNK_CHARS: usize = 4_000;
const MAX_RELEVANT_CHUNKS: i64 = 8;

// Phase 3 hybrid-retrieval fusion weights. `score = W_LEX·lexical_norm
// + W_VEC·cosine`. Lexical still leads (exact symbol/path hits are the
// strongest signal in code search); the vector term adds paraphrase /
// morphology recall. COS_FLOOR is the minimum cosine for a chunk with
// NO lexical hit to still be admitted (keeps pure-vector noise out).
const W_LEX: f32 = 0.55;
const W_VEC: f32 = 0.45;
const COS_FLOOR: f32 = 0.18;

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
            // Phase 3: store a per-chunk embedding alongside the text.
            // Embed path + content so file-name tokens contribute to
            // the vector too. NULL when nothing embeddable — retrieval
            // transparently falls back to lexical for that row.
            let embedding =
                crate::grounding::embedding::embed(&format!("{rel}\n{chunk}"));
            sqlx::query(
                "INSERT INTO project_file_chunks (project_file_id, project_id, path, chunk_index, content, embedding, indexed_at)
                 VALUES ($1, $2, $3, $4, $5, $6, NOW())",
            )
            .bind(file_id)
            .bind(project_id)
            .bind(&rel)
            .bind(chunk_index as i32)
            .bind(chunk)
            .bind(embedding)
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
    // Hybrid retrieval (Phase 3): lexical term scoring fused with
    // vector cosine. Either signal alone is enough to run — pure
    // lexical when there's no query embedding or chunks predate the
    // embedding column (NULL), pure/assisted vector otherwise. This is
    // why the migration could be additive and the rollout is safe:
    // un-reindexed projects behave exactly as the old lexical path.
    let terms = query_terms(query);
    let query_emb = crate::grounding::embedding::embed(query);
    if terms.is_empty() && query_emb.is_none() {
        return Ok(None);
    }

    let chunks: Vec<(String, i32, String, Option<Vec<f32>>)> = sqlx::query_as(
        "SELECT path, chunk_index, content, embedding
         FROM project_file_chunks
         WHERE project_id = $1
         ORDER BY indexed_at DESC
         LIMIT 800",
    )
    .bind(project_id)
    .fetch_all(db)
    .await?;

    // Pass 1: raw lexical score + raw cosine per candidate.
    let mut max_lex = 0i32;
    let mut candidates: Vec<(i32, f32, String, i32, String)> = Vec::new();
    for (path, idx, content, emb) in chunks {
        let lex = score_chunk(&path, &content, &terms);
        let cos = match (&query_emb, &emb) {
            (Some(q), Some(e)) => crate::grounding::embedding::cosine(q, e).max(0.0),
            _ => 0.0,
        };
        // Admit if lexically relevant OR vector-similar enough on its
        // own (paraphrase match with no shared tokens).
        if lex <= 0 && cos < COS_FLOOR {
            continue;
        }
        max_lex = max_lex.max(lex);
        candidates.push((lex, cos, path, idx, content));
    }

    if candidates.is_empty() {
        return Ok(None);
    }

    // Pass 2: normalise lexical to [0,1] by the batch max, then fuse.
    let mut scored: Vec<(f32, String, i32, String)> = candidates
        .into_iter()
        .map(|(lex, cos, path, idx, content)| {
            let lex_n = if max_lex > 0 {
                lex as f32 / max_lex as f32
            } else {
                0.0
            };
            let hybrid = W_LEX * lex_n + W_VEC * cos;
            (hybrid, path, idx, content)
        })
        .collect();

    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
    });
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
