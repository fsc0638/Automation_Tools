-- Snapshot of the KWay portal's "員工通訊錄" + the department list that
-- drives its dropdown. Two tables so:
--   * meetings (future) can autocomplete attendees against portal_employees
--     while still letting in-app users (NULL employee_no) coexist
--   * department / org-chart features can join on the canonical code
--
-- The portal is the source of truth — we re-import every Monday 08:00.
-- `last_seen_at` is bumped on every import a row appears in, so rows that
-- vanish from a future scrape can be aged out (or just flagged) without
-- deleting historical data.

CREATE TABLE portal_departments (
    id            UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    code          TEXT NOT NULL UNIQUE,        -- e.g. "A000", "B000"
    name          TEXT NOT NULL,               -- e.g. "董事長室"
    last_seen_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_portal_departments_name ON portal_departments(name);


CREATE TABLE portal_employees (
    id             UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    employee_no    TEXT NOT NULL UNIQUE,       -- e.g. "1668", "0337"
    name           TEXT NOT NULL,              -- e.g. "王崇旭"
    -- Portal lists extensions comma-separated when an employee has more
    -- than one (e.g. "101,102"). We store as a text array so the UI /
    -- future search can match any of them without re-parsing.
    extensions     TEXT[] NOT NULL DEFAULT '{}'::text[],
    email          TEXT,
    title          TEXT,                       -- 職稱 e.g. "資深專員"
    dept_code      TEXT,                       -- soft FK to portal_departments(code)
    -- When we can match the portal employee to an actual app user (same
    -- email), we link here so meetings can resolve attendee → user.
    -- NULL until matched; the importer maintains the link best-effort.
    user_id        UUID REFERENCES users(id) ON DELETE SET NULL,
    last_seen_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_portal_employees_dept   ON portal_employees(dept_code);
CREATE INDEX idx_portal_employees_email  ON portal_employees(LOWER(email));
CREATE INDEX idx_portal_employees_name   ON portal_employees(name);
CREATE INDEX idx_portal_employees_user   ON portal_employees(user_id);
