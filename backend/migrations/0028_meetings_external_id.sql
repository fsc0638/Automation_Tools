-- Tracking key for meetings imported from external systems (kway_portal,
-- and any future portal-side feeds). Format:
--   "<source-system>:<feature>:<natural-key>"
-- For kway_portal meeting-room imports:
--   "kway-portal:meeting-rooms:YYYY-MM-DD:Cxx:HHMM"
--
-- NULL means the meeting was created in-app (the existing flow). PostgreSQL
-- treats NULLs as distinct under UNIQUE, so multiple in-app meetings can
-- still coexist; only non-NULL external_id collisions are rejected.
ALTER TABLE meetings ADD COLUMN external_id TEXT;
CREATE UNIQUE INDEX idx_meetings_external_id ON meetings(external_id)
    WHERE external_id IS NOT NULL;
