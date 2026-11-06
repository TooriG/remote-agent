PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;

-- Public identities remain stable while their provider bindings change. Only
-- non-indexed nested data lives in payload_json; scalar state is never mirrored there.
CREATE TABLE IF NOT EXISTS threads (
    provider TEXT NOT NULL CHECK (provider IN ('codex', 'claude', 'deepseek')),
    id TEXT NOT NULL CHECK (length(id) BETWEEN 1 AND 4096 AND trim(id) = id),
    cwd TEXT,
    name TEXT,
    preview TEXT,
    -- Host titles/previews override observations without freezing native metadata on reload.
    name_override INTEGER NOT NULL DEFAULT 0 CHECK (name_override IN (0, 1)),
    preview_override INTEGER NOT NULL DEFAULT 0 CHECK (preview_override IN (0, 1)),
    registered INTEGER NOT NULL DEFAULT 0 CHECK (registered IN (0, 1)),
    imported INTEGER NOT NULL DEFAULT 0 CHECK (imported IN (0, 1)),
    active_provider TEXT,
    active_id TEXT,
    settled INTEGER NOT NULL DEFAULT 0 CHECK (settled IN (0, 1)),
    completed_turn_id TEXT,
    unread INTEGER NOT NULL DEFAULT 0 CHECK (unread IN (0, 1) AND (unread = 0 OR completed_turn_id IS NOT NULL)),
    list_revision INTEGER NOT NULL DEFAULT 0 CHECK (list_revision >= 0),
    payload_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(payload_json) AND json_type(payload_json) = 'object'),
    PRIMARY KEY (provider, id),
    FOREIGN KEY (provider, id, active_provider, active_id)
        REFERENCES provider_threads(thread_provider, thread_id, provider, id)
        DEFERRABLE INITIALLY DEFERRED,
    CHECK ((active_provider IS NULL) = (active_id IS NULL))
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS threads_settled ON threads (settled, provider, id);

CREATE TABLE IF NOT EXISTS provider_threads (
    provider TEXT NOT NULL CHECK (provider IN ('codex', 'claude', 'deepseek')),
    id TEXT NOT NULL,
    thread_provider TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    covered INTEGER CHECK (covered >= 0),
    PRIMARY KEY (provider, id),
    UNIQUE (thread_provider, thread_id, ordinal),
    UNIQUE (thread_provider, thread_id, provider, id),
    FOREIGN KEY (thread_provider, thread_id) REFERENCES threads(provider, id)
) STRICT, WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS turns (
    provider TEXT NOT NULL,
    id TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    turn_id TEXT NOT NULL,
    running INTEGER NOT NULL CHECK (running IN (0, 1)),
    body BLOB NOT NULL,
    PRIMARY KEY (provider, id, position),
    FOREIGN KEY (provider, id) REFERENCES threads(provider, id)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS turn_ids ON turns (provider, id, turn_id, position);

CREATE TABLE IF NOT EXISTS versions (
    provider TEXT NOT NULL,
    id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version >= 0),
    PRIMARY KEY (provider, id),
    FOREIGN KEY (provider, id) REFERENCES threads(provider, id)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS version_order ON versions (version);

CREATE TABLE IF NOT EXISTS events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    id TEXT NOT NULL,
    kind TEXT NOT NULL,
    occurred_at_ms INTEGER NOT NULL DEFAULT (CAST(unixepoch('subsec') * 1000 AS INTEGER)),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    FOREIGN KEY (provider, id) REFERENCES threads(provider, id)
) STRICT;
CREATE INDEX IF NOT EXISTS events_thread_sequence ON events(provider, id, sequence);

CREATE TABLE IF NOT EXISTS command_receipts (
    provider TEXT NOT NULL,
    id TEXT NOT NULL,
    command_id TEXT NOT NULL CHECK (length(command_id) BETWEEN 1 AND 256),
    kind TEXT NOT NULL,
    delivery TEXT NOT NULL CHECK (delivery IN ('sending', 'accepted', 'unknown', 'rejected')),
    turn_id TEXT,
    resolved INTEGER NOT NULL DEFAULT 0 CHECK (resolved IN (0, 1)),
    result_sequence INTEGER NOT NULL CHECK (result_sequence >= 0),
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    PRIMARY KEY (provider, id, command_id),
    FOREIGN KEY (provider, id) REFERENCES threads(provider, id)
) STRICT, WITHOUT ROWID;
