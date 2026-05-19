use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::db::models::Project;

const MAX_INDEX_FILE_BYTES: u64 = 1_000_000;
const MAX_INDEX_FILES: usize = 2_000;
const CHUNK_CHARS: usize = 4_000;
const MAX_RELEVANT_CHUNKS: i64 = 12;

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

/// Slugify a source label so it can safely namespace stored file paths
/// (only used when a workspace has >1 source, to avoid (project_id,
/// path) collisions between sources that share a relative path).
fn slug(label: &str) -> String {
    let s: String = label
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "src".into() } else { s }
}

/// Mirror of projects::project_root_path (kept local to avoid a cross-
/// module pub). Legacy single-source fallback when a project has no
/// project_sources rows yet.
fn legacy_root(p: &Project) -> String {
    if p.source_type == "git" || p.source_type == "upload" {
        p.local_path.clone().unwrap_or_else(|| p.source_path.clone())
    } else {
        p.source_path.clone()
    }
}

/// Full reindex of an entire workspace across ALL its sources
/// (migration 0040). Deletes the project's index once, then walks every
/// project_sources root. Single-source workspaces keep byte-identical
/// stored paths (no prefix); multi-source workspaces namespace each
/// source's paths by its slugged label so identical relative paths
/// across sources don't collide. Falls back to the legacy single
/// source column when the project has no project_sources rows.
pub async fn rebuild_project_index_all(db: &PgPool, project: &Project) -> Result<usize> {
    let sources: Vec<(String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT kind, source_path, local_path, label
           FROM project_sources WHERE project_id = $1
          ORDER BY created_at",
    )
    .bind(project.id)
    .fetch_all(db)
    .await
    .unwrap_or_default();

    let roots: Vec<(String, String)> = if sources.is_empty() {
        let r = legacy_root(project);
        if r.trim().is_empty() { vec![] } else { vec![(String::new(), r)] }
    } else {
        sources
            .into_iter()
            .filter_map(|(kind, sp, lp, label)| {
                let root = if kind == "git" {
                    lp.filter(|s| !s.trim().is_empty()).unwrap_or(sp)
                } else {
                    sp
                };
                (!root.trim().is_empty()).then_some((label, root))
            })
            .collect()
    };

    // Delete the whole project's index ONCE, then re-add every source.
    sqlx::query("DELETE FROM project_file_chunks WHERE project_id = $1")
        .bind(project.id)
        .execute(db)
        .await?;
    sqlx::query("DELETE FROM project_files WHERE project_id = $1")
        .bind(project.id)
        .execute(db)
        .await?;

    let multi = roots.len() > 1;
    let mut total = 0usize;
    for (label, root) in roots {
        let root_path = Path::new(&root);
        if !root_path.exists() {
            continue; // a missing source must not abort the others
        }
        let prefix = if multi { format!("{}/", slug(&label)) } else { String::new() };
        total += index_one_root(db, project.id, root_path, &prefix).await?;
    }
    Ok(total)
}

/// Walk one root and insert its files/chunks. Does NOT delete (callers
/// own the delete so multi-source can clear once then add many).
/// `path_prefix` is prepended to every stored path (empty for single
/// source, "<slug>/" when namespacing multiple sources).
async fn index_one_root(
    db: &PgPool,
    project_id: Uuid,
    root_path: &Path,
    path_prefix: &str,
) -> Result<usize> {
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
        let rel = format!(
            "{path_prefix}{}",
            file.strip_prefix(root_path)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/")
        );
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
            // ONNX inference is CPU-heavy and synchronous. Run it on
            // the blocking pool so a big (multi-source) reindex never
            // starves tokio workers / the live chat stream.
            let embed_input = format!("{rel}\n{chunk}");
            let embedding = tokio::task::spawn_blocking(move || {
                crate::grounding::embedding::embed(&embed_input)
            })
            .await
            .ok()
            .flatten();
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
    // Lexical terms = raw query tokens + zh→en developer-intent
    // expansion (the precision layer that lifts the English source
    // files a Chinese question is really about, since the model's
    // NL↔code cosine is too flat to rank them itself).
    let mut terms = query_terms(query);
    for it in intent_terms(query) {
        if !terms.contains(&it) {
            terms.push(it);
        }
    }
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
        // Lock/min/map files are never code evidence and only crowd
        // out real answers in the top-K — drop before scoring.
        if is_hard_noise_path(&path) {
            continue;
        }
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
        } else if file_type.is_file()
            && is_text_file(&path)
            && path.starts_with(root)
            && !crate::grounding::is_secret_path(&name)
        {
            // Hardening: secret files (*.env, *.pem, id_rsa, …) are
            // never chunked/embedded. Source code stays indexed (it's
            // "readable-but-redacted"); pure-secret files don't belong
            // in the corpus at all.
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

/// Files that are never useful as "code evidence" and only crowd out
/// real answers in the top-K (the diagnostic showed package-lock.json
/// out-ranking sessions.py). Hard-skipped from retrieval candidates.
/// Indexing is unchanged (additive, no reindex needed) — this is a
/// pure retrieval-time filter.
fn is_hard_noise_path(path: &str) -> bool {
    let p = path.to_lowercase();
    let name = p.rsplit('/').next().unwrap_or(&p);
    name.ends_with(".min.js")
        || name.ends_with(".min.css")
        || name.ends_with(".map")
        || name.ends_with(".lock")
        || matches!(
            name,
            "package-lock.json"
                | "pnpm-lock.yaml"
                | "yarn.lock"
                | "cargo.lock"
                | "poetry.lock"
                | "composer.lock"
                | "gemfile.lock"
        )
}

/// Compact zh→en developer-intent lexicon. The multilingual model gives
/// *recall* but its NL-question↔code cosine is flat (0.39–0.43 for
/// everything, the diagnostic proved), so it ranks READMEs above code.
/// These English keywords are folded into the lexical terms so a
/// Chinese question can score-boost the English source files whose PATH
/// or content actually implements that concept (e.g. 登入 → the path
/// `app/core/sessions.py` / `tests/test_auth.py`). Precision layer
/// stacked ON TOP of the model the user chose, not a replacement.
fn intent_terms(query: &str) -> Vec<String> {
    const MAP: &[(&str, &[&str])] = &[
        ("登入", &["login", "signin", "auth", "authenticate", "session"]),
        ("登錄", &["login", "signin", "auth", "session"]),
        ("登出", &["logout", "signout", "session"]),
        ("註冊", &["register", "signup", "registration"]),
        ("驗證", &["auth", "verify", "validate", "token", "credential"]),
        ("認證", &["auth", "authenticate", "credential", "token"]),
        ("授權", &["authorize", "authz", "permission", "scope", "token"]),
        ("權限", &["permission", "role", "acl", "authz", "access"]),
        ("角色", &["role", "permission"]),
        ("流程", &["flow", "pipeline", "process", "workflow"]),
        ("會議", &["meeting"]),
        ("同步", &["sync", "synchronize"]),
        ("接地", &["grounding", "context", "retrieval"]),
        ("檢索", &["retrieval", "search", "index", "embedding"]),
        ("資料庫", &["db", "database", "sql", "migration", "schema", "model"]),
        ("資料表", &["table", "schema", "model", "migration"]),
        ("設定", &["config", "settings", "env"]),
        ("組態", &["config", "settings"]),
        ("錯誤", &["error", "exception", "fail", "panic"]),
        ("例外", &["exception", "error"]),
        ("測試", &["test", "spec"]),
        ("路由", &["route", "router", "endpoint", "handler"]),
        ("接口", &["api", "endpoint", "interface", "handler"]),
        ("介面", &["api", "interface", "ui", "component"]),
        ("前端", &["frontend", "ui", "component", "page"]),
        ("後端", &["backend", "server", "service"]),
        ("金鑰", &["key", "secret", "token", "credential"]),
        ("密鑰", &["key", "secret", "token", "credential"]),
        ("憑證", &["credential", "cert", "token"]),
        ("使用者", &["user", "account"]),
        ("帳號", &["account", "user"]),
        ("密碼", &["password", "credential", "hash"]),
        ("通知", &["notification", "notify"]),
        ("排程", &["schedule", "cron", "job"]),
        ("快取", &["cache"]),
        ("上傳", &["upload"]),
        ("下載", &["download"]),
    ];
    let mut out: Vec<String> = Vec::new();
    for (zh, ens) in MAP {
        if query.contains(zh) {
            for e in *ens {
                let e = e.to_string();
                if !out.contains(&e) {
                    out.push(e);
                }
            }
        }
    }
    out
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

#[cfg(test)]
mod diag {
    use super::*;

    /// Diagnostic (run explicitly): connect to the live DB, take the
    /// exact failing query, and print the top chunks by lexical /
    /// cosine / hybrid for a project — so we can SEE whether the right
    /// files are retrievable and at what score (is COS_FLOOR eating
    /// everything?), instead of guessing.
    ///
    ///   DATABASE_URL=postgres://postgres:login123@localhost:5432/kway_dev \
    ///   PROJECT_ID=7c214d1a-c6f6-4d4a-b4c9-85a49d49b537 \
    ///   cargo test --release --bin kway-dev-backend \
    ///     api::project_index::diag::retrieval -- --ignored --nocapture
    #[ignore]
    #[tokio::test]
    async fn retrieval() {
        let db_url = std::env::var("DATABASE_URL").expect("set DATABASE_URL");
        let project_id: Uuid = std::env::var("PROJECT_ID")
            .expect("set PROJECT_ID")
            .parse()
            .expect("PROJECT_ID must be a uuid");
        let query = std::env::var("Q")
            .unwrap_or_else(|_| "這專案登入流程怎麼寫的".to_string());

        let db = sqlx::PgPool::connect(&db_url).await.expect("db connect");
        let mut terms = query_terms(&query);
        for it in intent_terms(&query) {
            if !terms.contains(&it) {
                terms.push(it);
            }
        }
        let qemb = crate::grounding::embedding::embed(&query)
            .expect("query embed (model must be available)");

        let rows: Vec<(String, i32, String, Option<Vec<f32>>)> = sqlx::query_as(
            "SELECT path, chunk_index, content, embedding
             FROM project_file_chunks WHERE project_id = $1
             ORDER BY indexed_at DESC LIMIT 800",
        )
        .bind(project_id)
        .fetch_all(&db)
        .await
        .expect("query chunks");

        println!(
            "query={query:?}\nterms={terms:?}\nchunks={} with_emb={}",
            rows.len(),
            rows.iter().filter(|r| r.3.is_some()).count()
        );

        // Replicate production relevant_file_context scoring exactly.
        let mut max_lex = 0i32;
        let mut cands: Vec<(i32, f32, String, i32)> = Vec::new();
        for (path, idx, content, emb) in &rows {
            if is_hard_noise_path(path) {
                continue;
            }
            let lex = score_chunk(path, content, &terms);
            let cos = match emb {
                Some(e) => crate::grounding::embedding::cosine(&qemb, e).max(0.0),
                None => 0.0,
            };
            if lex <= 0 && cos < COS_FLOOR {
                continue;
            }
            max_lex = max_lex.max(lex);
            cands.push((lex, cos, path.clone(), *idx));
        }
        let mut scored: Vec<(f32, i32, f32, String, i32)> = cands
            .into_iter()
            .map(|(lex, cos, path, idx)| {
                let lex_n = if max_lex > 0 {
                    lex as f32 / max_lex as f32
                } else {
                    0.0
                };
                (W_LEX * lex_n + W_VEC * cos, lex, cos, path, idx)
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

        println!(
            "--- FINAL TOP {} the AI actually receives (hybrid) ---",
            MAX_RELEVANT_CHUNKS
        );
        for (h, lex, cos, path, idx) in scored.iter().take(MAX_RELEVANT_CHUNKS as usize) {
            println!("hybrid={h:.3} lex={lex:>3} cos={cos:.3} {path}#{idx}");
        }
        let auth_rank = scored.iter().position(|(_, _, _, p, _)| {
            let p = p.to_lowercase();
            p.contains("auth") || p.contains("session") || p.contains("login")
        });
        println!(
            "admitted={}  first auth/session/login file rank={:?} (want < {})",
            scored.len(),
            auth_rank,
            MAX_RELEVANT_CHUNKS
        );
    }
}
