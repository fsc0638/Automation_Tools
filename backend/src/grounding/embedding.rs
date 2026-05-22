//! Phase 3 — local cross-lingual semantic embedding (fastembed-rs).
//!
//! ## Why this exists / the decision trail
//!
//! Phase 0 spike: gateway embedding endpoints are dead and pgvector is
//! unavailable on PG18, so vectors live in a `REAL[]` column scored by
//! Rust-side cosine. The first Phase 3 cut used a zero-dependency
//! hashed n-gram vector to dodge the heavy ONNX dependency in this
//! WDAC-locked/offline box. **Field test killed that**: a Chinese
//! question ("這專案登入流程怎麼寫的") against an English/Python
//! codebase shares no tokens *or* character n-grams, so the hashed
//! vector (like pure lexical) retrieved nothing — the AI only saw a
//! file tree and asked the user to paste the files.
//!
//! User decision (2026-05-18): accept the heavy dependency and run a
//! real **multilingual** model so zh↔en semantic retrieval actually
//! works. This module now wraps `fastembed` (local ONNX, no network at
//! inference time once the model is cached).
//!
//! ## The fail-open invariant is unchanged
//!
//! The plan's hard rule still holds: "embedding provider 不可用時自動
//! 退回純字面（不阻斷）". Model init is lazy and fault-tolerant — if
//! the ONNX runtime or the model files are missing (offline first run,
//! WDAC blocks the native lib, etc.) `embed` returns `None` and
//! `relevant_file_context` transparently degrades to lexical scoring.
//! Nothing ever blocks or panics on the embedding path.

use std::sync::{Mutex, OnceLock};

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

/// Nominal embedding dimensionality (paraphrase-multilingual-MiniLM-L12
/// = 384). Informational only: the DB column is an unbounded `REAL[]`
/// and `cosine` guards length mismatches, so a model swap just needs a
/// re-index — old-dimension vectors simply contribute no signal until
/// then instead of corrupting ranking.
#[allow(dead_code)]
pub const EMBED_DIM: usize = 384;

/// Lazily-initialised, process-wide embedder.
///
/// `Option` is the fail-open switch: `None` means "no embedder this
/// process" (init failed) and every `embed` call cheaply returns
/// `None`. `Mutex` serialises inference — the ONNX session is not
/// guaranteed `Sync` for concurrent `embed`, and retrieval/indexing
/// are not hot enough to need a pool.
static EMBEDDER: OnceLock<Option<Mutex<TextEmbedding>>> = OnceLock::new();

/// Absolute, OneDrive-free model cache. fastembed's default is
/// `./.fastembed_cache` relative to cwd — here that resolves *inside*
/// the OneDrive-synced repo (~465 MB the model weighs), which OneDrive
/// would thrash and git could swallow. Pin it to the same non-synced
/// `C:\rust-build\…` area this project already uses for the WDAC build
/// override. Overridable via `FASTEMBED_CACHE_DIR`.
fn cache_dir() -> std::path::PathBuf {
    std::env::var("FASTEMBED_CACHE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(r"C:\rust-build\kway-fastembed-cache"))
}

fn embedder() -> &'static Option<Mutex<TextEmbedding>> {
    EMBEDDER.get_or_init(|| {
        // Symmetric multilingual model: no asymmetric query/passage
        // prefix needed, so a single `embed` fn serves both indexing
        // and querying. First call downloads + caches the model; later
        // calls are fully local.
        match TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::ParaphraseMLMiniLML12V2)
                .with_show_download_progress(false)
                .with_cache_dir(cache_dir()),
        ) {
            Ok(model) => {
                tracing::info!("grounding embedder ready (ParaphraseMLMiniLML12V2)");
                Some(Mutex::new(model))
            }
            Err(e) => {
                // The whole point of fail-open: log once, never panic,
                // let the caller fall back to lexical retrieval.
                tracing::warn!(
                    error = %e,
                    "grounding embedder unavailable — falling back to lexical-only retrieval"
                );
                None
            }
        }
    })
}

/// Produce a semantic embedding for `text`, or `None` when there is
/// nothing to embed or the embedder is unavailable (⇒ caller uses
/// lexical only). Never panics.
pub fn embed(text: &str) -> Option<Vec<f32>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let model = embedder().as_ref()?;
    let guard = model.lock().ok()?;
    // fastembed batches; we embed one document at a time (call sites are
    // per-chunk / per-query). `None` = library default batch size.
    let mut out = guard.embed(vec![trimmed.to_string()], None).ok()?;
    let v = out.pop()?;
    if v.is_empty() {
        return None;
    }
    Some(v)
}

/// Cosine similarity of two embeddings. fastembed output is already
/// L2-normalised, so this is the dot product; we still guard length and
/// degenerate cases and clamp to [-1, 1]. **Mismatched lengths** (e.g.
/// an embedding written under a previous model/`EMBED_DIM`) return 0.0
/// so that row simply contributes no vector signal rather than
/// corrupting ranking — this is what makes a model swap safe pre-reindex.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    dot.clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Proves the real point of switching to fastembed: a Chinese
    /// question is semantically closer to the matching English code
    /// concept than to an unrelated one. Also doubles as the
    /// environment check — if the model can't be fetched in this
    /// WDAC/offline box, `embed` returns None and the asserts make
    /// that explicit (rather than silently shipping fail-open lexical).
    #[test]
    fn chinese_query_matches_english_code_semantics() {
        let zh = embed("這專案的登入驗證流程怎麼實作的");
        let en_match = embed("user login authentication and session verification flow");
        let en_unrelated = embed("generate a PDF invoice and email it to the customer");

        let (zh, en_match, en_unrelated) = match (zh, en_match, en_unrelated) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => panic!(
                "embedder unavailable in this environment — model could \
                 not be initialised (offline/WDAC?). Cross-lingual \
                 retrieval would fall back to lexical."
            ),
        };

        let sim_match = cosine(&zh, &en_match);
        let sim_unrelated = cosine(&zh, &en_unrelated);
        println!("zh↔login={sim_match:.3}  zh↔unrelated={sim_unrelated:.3}");
        assert!(
            sim_match > sim_unrelated,
            "expected the login sentence to be closer to the Chinese \
             question than the unrelated one (got match={sim_match:.3} \
             vs unrelated={sim_unrelated:.3})"
        );
        assert!(
            sim_match > 0.30,
            "cross-lingual similarity too low ({sim_match:.3}) — model \
             may be wrong/not multilingual"
        );
    }
}
