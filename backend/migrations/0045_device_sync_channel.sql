-- Device sync channel: private Mac Mini <-> mobile/desktop agent bridge.
--
-- Devices are owned by users. Sync events are durable, append-only messages
-- scoped to the owning user and optionally a target/source device. Realtime WS
-- delivery is an optimization; clients can always replay from `sequence_id`.

CREATE TABLE IF NOT EXISTS user_devices (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    owner_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    display_name TEXT NOT NULL,
    platform TEXT NOT NULL CHECK (platform IN ('macos','ios','android','web','agent','unknown')),
    device_public_key TEXT,
    trusted BOOLEAN NOT NULL DEFAULT FALSE,
    last_seen_at TIMESTAMPTZ,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_user_devices_owner
    ON user_devices(owner_user_id, updated_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_user_devices_owner_name
    ON user_devices(owner_user_id, lower(display_name));

CREATE TABLE IF NOT EXISTS device_sync_events (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    sequence_id BIGSERIAL UNIQUE NOT NULL,
    owner_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source_device_id UUID REFERENCES user_devices(id) ON DELETE SET NULL,
    target_device_id UUID REFERENCES user_devices(id) ON DELETE SET NULL,
    event_type TEXT NOT NULL CHECK (event_type IN (
        'file_changed','file_deleted','git_ref_updated','task_update','agent_message','presence','command','command_result'
    )),
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    payload_hash TEXT,
    delivery_state TEXT NOT NULL DEFAULT 'queued'
        CHECK (delivery_state IN ('queued','delivered','acked','failed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    delivered_at TIMESTAMPTZ,
    acked_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_device_sync_events_owner_sequence
    ON device_sync_events(owner_user_id, sequence_id DESC);
CREATE INDEX IF NOT EXISTS idx_device_sync_events_target_state
    ON device_sync_events(target_device_id, delivery_state, sequence_id DESC);
CREATE INDEX IF NOT EXISTS idx_device_sync_events_source
    ON device_sync_events(source_device_id, sequence_id DESC);

CREATE TABLE IF NOT EXISTS device_sync_cursors (
    device_id UUID PRIMARY KEY REFERENCES user_devices(id) ON DELETE CASCADE,
    last_sequence_id BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS device_sync_audit (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    owner_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id UUID REFERENCES user_devices(id) ON DELETE SET NULL,
    event_id UUID REFERENCES device_sync_events(id) ON DELETE SET NULL,
    operation TEXT NOT NULL CHECK (operation IN ('register','heartbeat','publish','deliver','ack','replay','trust_changed')),
    ip_addr TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_device_sync_audit_owner
    ON device_sync_audit(owner_user_id, created_at DESC);

COMMENT ON TABLE user_devices IS
    'User-owned devices allowed to participate in the private sync channel.';
COMMENT ON TABLE device_sync_events IS
    'Durable replay log for Mac Mini, phone, desktop, and agent sync messages.';
COMMENT ON COLUMN device_sync_events.sequence_id IS
    'Monotonic replay cursor. WebSocket delivery is best-effort; clients resume from this value.';
