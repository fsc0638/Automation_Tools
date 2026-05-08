-- Allow uploaded zip projects and add a lightweight lexical file index.
ALTER TABLE projects DROP CONSTRAINT IF EXISTS projects_source_type_check;
ALTER TABLE projects ADD CONSTRAINT projects_source_type_check CHECK (source_type IN ('local', 'git', 'upload'));

CREATE TABLE project_files (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    language TEXT,
    size_bytes BIGINT NOT NULL DEFAULT 0,
    content_hash TEXT NOT NULL,
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(project_id, path)
);

CREATE INDEX idx_project_files_project_id ON project_files(project_id);
CREATE INDEX idx_project_files_path ON project_files(project_id, path);

CREATE TABLE project_file_chunks (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_file_id UUID NOT NULL REFERENCES project_files(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(project_file_id, chunk_index)
);

CREATE INDEX idx_project_file_chunks_project_id ON project_file_chunks(project_id);
CREATE INDEX idx_project_file_chunks_path ON project_file_chunks(project_id, path);
