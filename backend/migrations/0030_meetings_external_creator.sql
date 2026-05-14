-- Surface the original portal booker's display name (Chinese name from
-- `booking.user`, e.g. "張淑芬") on portal-imported meetings. We can't
-- reliably link those bookers to local `users` rows (they may not have
-- registered yet), so we keep the raw string alongside `creator_id` and
-- let the list endpoint prefer it when present.

ALTER TABLE meetings
    ADD COLUMN IF NOT EXISTS external_creator_name TEXT;

COMMENT ON COLUMN meetings.external_creator_name IS
    'Portal scrape booker display name; null for app-created meetings.';
