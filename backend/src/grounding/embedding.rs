//! Phase 3 — local, zero-dependency, zero-network text embedding.
//!
//! ## Why not fastembed-rs / a gateway
//!
//! Phase 0 spike proved both gateway embedding endpoints are dead
//! (Hermes 404 / OpenClaw 500) and pgvector is unavailable on this
//! PG18 instance. The plan's hard, repeated constraint is:
//!
//! > embedding provider 不可用時**自動退回純字面**（不阻斷）
//!
//! A heavy ONNX embedder (fastembed-rs pulls `ort` + a HuggingFace
//! model download on first run) is exactly the kind of thing that, in
//! this WDAC-locked offline Windows environment, would be "unavailable"
//! — and it would also risk the build. So the default embedder here is
//! a **deterministic hashed n-gram bag-of-features vector**: it needs
//! no native lib, no model, no network, and therefore can never be the
//! "provider unavailable" case. It is a genuine vector space (cosine
//! over L2-normalised feature vectors) fused with the lexical score in
//! `relevant_file_context`, giving real hybrid retrieval today.
//!
//! The function boundary (`embed` / `cosine`) is the seam: a learned
//! embedder can replace `embed` later without touching any caller.

/// Embedding dimensionality. Small enough that storing one `REAL[]` per
/// chunk and doing Rust-side cosine over a few hundred candidates is
/// cheap; large enough to keep hashed-feature collisions low.
pub const EMBED_DIM: usize = 256;

/// FNV-1a 64-bit. Hand-rolled so the hash is byte-for-byte stable
/// regardless of std/compiler version (embeddings persist in the DB and
/// are compared across process restarts).
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

/// Tokenise into weighted features:
/// - whole alphanumeric/`_`/`-` tokens (lower-cased, len >= 2)
/// - CJK character bigrams (CJK has no spaces; bigrams capture phrases)
/// Each feature is hashed into the vector; this gives morphology- and
/// paraphrase-tolerant recall the pure-substring lexical score misses.
fn features(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut feats: Vec<String> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && !is_cjk(c))
        .map(|s| s.trim())
        .filter(|s| s.chars().count() >= 2)
        .map(|s| s.to_string())
        .collect();

    let cjk: Vec<char> = lower.chars().filter(|c| is_cjk(*c)).collect();
    for w in cjk.windows(2) {
        feats.push(w.iter().collect());
    }
    feats
}

/// Produce an L2-normalised embedding for `text`, or `None` when there
/// is nothing to embed (empty / no usable features). `None` is the
/// natural "no vector signal" case — callers fall back to lexical.
pub fn embed(text: &str) -> Option<Vec<f32>> {
    let feats = features(text);
    if feats.is_empty() {
        return None;
    }
    let mut v = vec![0.0f32; EMBED_DIM];
    for f in &feats {
        let h = fnv1a(f.as_bytes());
        let idx = (h as usize) % EMBED_DIM;
        // Signed bucket: spreads collisions instead of always adding.
        let sign = if (h >> 63) & 1 == 1 { -1.0 } else { 1.0 };
        v[idx] += sign;
    }
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm <= f32::EPSILON {
        return None;
    }
    for x in &mut v {
        *x /= norm;
    }
    Some(v)
}

/// Cosine similarity of two equal-length L2-normalised vectors. Since
/// `embed` already normalises, this is just the dot product; we guard
/// length/degenerate cases and clamp to [-1, 1]. Mismatched lengths
/// (e.g. an embedding from an older EMBED_DIM) return 0.0 so the row
/// simply contributes no vector signal rather than corrupting ranking.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    dot.clamp(-1.0, 1.0)
}
