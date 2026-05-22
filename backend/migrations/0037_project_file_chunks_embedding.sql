-- Phase 3 (hybrid retrieval). Additive: per-chunk embedding vector
-- stored as a plain REAL[] (float4[]) column. Phase 0 spike proved
-- pgvector is unavailable on this PostgreSQL 18 instance
-- (`extension "vector" is not available`), so we deliberately use a
-- native array + Rust-side cosine instead of pgvector.
--
-- NULLABLE on purpose: existing chunks (and any chunk whose embedding
-- could not be produced) keep embedding = NULL and the retrieval path
-- transparently falls back to pure lexical for them. Nothing is
-- modified or removed — old lexical-only behaviour is fully preserved
-- until a project is re-indexed.

ALTER TABLE project_file_chunks
    ADD COLUMN IF NOT EXISTS embedding REAL[];
