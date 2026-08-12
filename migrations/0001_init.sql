CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    is_admin      INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE invite_codes (
    id         INTEGER PRIMARY KEY,
    code       TEXT NOT NULL UNIQUE,
    created_by INTEGER NOT NULL REFERENCES users(id),
    used_by    INTEGER REFERENCES users(id),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    used_at    TEXT
);

CREATE TABLE sessions (
    token_hash BLOB PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at TEXT NOT NULL
);
CREATE INDEX idx_sessions_user ON sessions(user_id);

CREATE TABLE api_tokens (
    id           INTEGER PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    token_hash   BLOB NOT NULL UNIQUE,
    token_prefix TEXT NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at TEXT
);
CREATE INDEX idx_api_tokens_user ON api_tokens(user_id);

CREATE TABLE documents (
    id            INTEGER PRIMARY KEY,
    public_id     TEXT NOT NULL UNIQUE,
    owner_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    slug          TEXT NOT NULL,
    title         TEXT,
    published     INTEGER NOT NULL DEFAULT 0,
    password_hash TEXT,
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (owner_id, slug),
    CHECK (published = 0 OR password_hash IS NOT NULL)
);
CREATE INDEX idx_documents_owner ON documents(owner_id);

CREATE TABLE revisions (
    id          INTEGER PRIMARY KEY,
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    revision    INTEGER NOT NULL,
    html        BLOB NOT NULL,
    size_bytes  INTEGER NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (document_id, revision)
);
