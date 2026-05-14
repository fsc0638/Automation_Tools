-- P1: Core meeting tables.
-- Meetings can optionally link to a project (project_id) so they show up
-- under that project's history, but they are user-scoped by creator_id so
-- a meeting can also exist independently of any project.

CREATE TABLE meetings (
    id                  UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    creator_id          UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    organization_id     UUID REFERENCES organizations(id) ON DELETE SET NULL,
    project_id          UUID REFERENCES projects(id) ON DELETE SET NULL,
    title               TEXT NOT NULL,
    importance          TEXT NOT NULL DEFAULT 'normal'
                            CHECK (importance IN ('normal', 'important')),
    start_at            TIMESTAMPTZ NOT NULL,
    end_at              TIMESTAMPTZ NOT NULL,
    all_day             BOOLEAN NOT NULL DEFAULT FALSE,
    recurrence          TEXT NOT NULL DEFAULT 'none'
                            CHECK (recurrence IN ('none', 'daily', 'weekly', 'monthly')),
    timezone            TEXT NOT NULL DEFAULT 'Asia/Taipei',
    location            TEXT,
    notification_note   TEXT,
    status              TEXT NOT NULL DEFAULT 'draft'
                            CHECK (status IN
                              ('draft','scheduled','in_progress','completed','cancelled')),
    invitations_sent_at TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_meetings_creator  ON meetings(creator_id, start_at);
CREATE INDEX idx_meetings_project  ON meetings(project_id, start_at);
CREATE INDEX idx_meetings_org_date ON meetings(organization_id, start_at);
CREATE INDEX idx_meetings_status   ON meetings(status, start_at);

-- Attendees: support both registered users (user_id) and external email
-- invitees. PRIMARY KEY uses (meeting_id, email) so the same email can
-- be invited to many meetings without collision.
CREATE TABLE meeting_attendees (
    meeting_id          UUID NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    user_id             UUID REFERENCES users(id) ON DELETE SET NULL,
    email               TEXT NOT NULL,
    display_name        TEXT NOT NULL DEFAULT '',
    role_label          TEXT,
    confirmation_status TEXT NOT NULL DEFAULT 'pending'
                            CHECK (confirmation_status IN
                              ('pending','confirmed','disputed')),
    confirmed_at        TIMESTAMPTZ,
    dispute_note        TEXT,
    last_action_at      TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (meeting_id, email)
);
CREATE INDEX idx_meeting_attendees_user ON meeting_attendees(user_id);

-- Files: attachments, recordings, transcripts all live here, differentiated
-- by file_category. storage_path is relative to project_data_root.
CREATE TABLE meeting_files (
    id               UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    meeting_id       UUID NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    uploader_id      UUID NOT NULL REFERENCES users(id),
    filename         TEXT NOT NULL,
    storage_path     TEXT NOT NULL,
    file_size        BIGINT NOT NULL DEFAULT 0,
    mime_type        TEXT NOT NULL DEFAULT 'application/octet-stream',
    file_category    TEXT NOT NULL DEFAULT 'attachment'
                         CHECK (file_category IN ('attachment','recording','transcript')),
    upload_status    TEXT NOT NULL DEFAULT 'uploaded'
                         CHECK (upload_status IN ('pending','uploaded','processing','failed')),
    duration_seconds INT,
    transcript_meta  TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_meeting_files_meeting ON meeting_files(meeting_id);
