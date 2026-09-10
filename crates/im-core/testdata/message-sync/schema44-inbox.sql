-- Frozen schema-44 receive table for the versioned upgrade tests.
CREATE TABLE IF NOT EXISTS sync_lane_inbox (
    input_id                    TEXT PRIMARY KEY,
    owner_identity_id           TEXT NOT NULL,
    lane                        TEXT NOT NULL,
    lane_epoch                  TEXT NOT NULL,
    position                    TEXT NOT NULL,
    event_id                    TEXT NOT NULL,
    event_type                  TEXT NOT NULL,
    raw_payload_json            TEXT NOT NULL,
    payload_bytes               INTEGER NOT NULL,
    account_id_snapshot         TEXT NOT NULL,
    device_id_snapshot          TEXT NOT NULL,
    auth_generation_snapshot    TEXT NOT NULL,
    client_instance_id_snapshot TEXT NOT NULL,
    group_did                   TEXT,
    received_at                 TEXT NOT NULL,
    source_created_at           TEXT,
    source_expires_at           TEXT,
    closed_at                   INTEGER,
    created_at                  INTEGER NOT NULL,
    UNIQUE (owner_identity_id, lane, lane_epoch, event_id),
    UNIQUE (owner_identity_id, lane, lane_epoch, position),
    CHECK (lane IN ('p5_device', 'p6_group')),
    CHECK (event_type IN (
        'p5.delivery.created', 'p6.delivery.created', 'p6.control.notice'
    )),
    CHECK (json_valid(raw_payload_json)),
    CHECK (json_type(raw_payload_json) = 'object'),
    CHECK (payload_bytes > 0 AND payload_bytes <= 1048576),
    CHECK (length(trim(input_id)) > 0),
    CHECK (length(trim(event_id)) > 0),
    CHECK (length(trim(received_at)) > 0),
    CHECK (
        lane_epoch <> ''
        AND lane_epoch NOT GLOB '*[^0-9]*'
        AND substr(lane_epoch, 1, 1) <> '0'
    ),
    CHECK (
        position <> ''
        AND position NOT GLOB '*[^0-9]*'
        AND substr(position, 1, 1) <> '0'
    ),
    CHECK (
        (lane = 'p5_device' AND event_type = 'p5.delivery.created' AND group_did IS NULL)
        OR
        (lane = 'p6_group' AND event_type IN (
            'p6.delivery.created', 'p6.control.notice'
        ) AND length(trim(group_did)) > 0)
    ),
    FOREIGN KEY (owner_identity_id)
        REFERENCES identity_account_bindings(owner_identity_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_sync_lane_inbox_pending
ON sync_lane_inbox(owner_identity_id, lane, closed_at, created_at, input_id);

CREATE INDEX IF NOT EXISTS idx_sync_lane_inbox_closed_gc
ON sync_lane_inbox(closed_at)
WHERE closed_at IS NOT NULL;

