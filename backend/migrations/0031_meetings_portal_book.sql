-- Track whether a meeting has been pushed to crm.kway.com.tw's "預約會議室"
-- form. A booking is owned by us once we (the service account) succeed on
-- conference_mgr.jsp / conference_mgrM.jsp. The portal does not return a
-- stable id we can store, so the reservation is keyed implicitly by
-- (room, date, time) — the same tuple we send to conference_mgrD.jsp on
-- cancel.
--
-- `portal_book_error` keeps the last failure message so the UI can show
-- "為什麼這場還是 draft" without making the operator dig into log files.
-- A successful re-try clears it.

ALTER TABLE meetings
    ADD COLUMN IF NOT EXISTS portal_booked_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS portal_book_error TEXT;

COMMENT ON COLUMN meetings.portal_booked_at IS
    'When this meeting was successfully pushed to the KWay portal. NULL = not yet booked / cancelled.';

COMMENT ON COLUMN meetings.portal_book_error IS
    'Last portal-side failure message (cleared on success). NULL when booking succeeded or was never attempted.';
