-- Meeting AI notes (versioned), edit-history audit trail, and the link
-- between meetings and project tasks they affect.

CREATE TABLE meeting_notes (
    id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    meeting_id          UUID NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    version             INT NOT NULL DEFAULT 1,
    summary             TEXT,
    decisions           JSONB NOT NULL DEFAULT '[]'::jsonb,
    risks               JSONB NOT NULL DEFAULT '[]'::jsonb,
    transcript_excerpts JSONB NOT NULL DEFAULT '[]'::jsonb,
    generated_by        TEXT NOT NULL DEFAULT 'ai',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (meeting_id, version)
);
CREATE INDEX idx_meeting_notes_meeting ON meeting_notes(meeting_id);

-- Edit history: each manual update writes a new snapshot row so the UI
-- can render "v1 → v2 → v3 with editor + edit_summary" timeline.
CREATE TABLE meeting_notes_edits (
    id           UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    meeting_id   UUID NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    version      INT NOT NULL,
    edited_by    UUID NOT NULL REFERENCES users(id),
    edit_summary TEXT NOT NULL DEFAULT '',
    snapshot     JSONB NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_meeting_notes_edits ON meeting_notes_edits(meeting_id, created_at DESC);

-- Meeting → task impact links. task_id may be NULL when the meeting
-- mentions a task that hasn't been created yet (description text only).
CREATE TABLE meeting_task_impacts (
    id            UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    meeting_id    UUID NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    project_id    UUID REFERENCES projects(id) ON DELETE CASCADE,
    task_id       UUID REFERENCES project_tasks(id) ON DELETE SET NULL,
    impact_type   TEXT NOT NULL
                      CHECK (impact_type IN ('new','update','progress')),
    description   TEXT NOT NULL,
    progress_from INT,
    progress_to   INT,
    is_hidden     BOOLEAN NOT NULL DEFAULT FALSE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_meeting_task_impacts_meeting ON meeting_task_impacts(meeting_id);
CREATE INDEX idx_meeting_task_impacts_task    ON meeting_task_impacts(task_id);
