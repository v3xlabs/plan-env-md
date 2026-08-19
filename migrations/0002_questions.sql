CREATE TABLE revision_questions (
    id          INTEGER PRIMARY KEY,
    revision_id INTEGER NOT NULL REFERENCES revisions(id) ON DELETE CASCADE,
    ord         INTEGER NOT NULL,
    key         TEXT NOT NULL,
    data        TEXT NOT NULL,
    UNIQUE (revision_id, key),
    UNIQUE (revision_id, ord)
);
CREATE INDEX idx_revision_questions_revision ON revision_questions(revision_id);

-- No foreign key to revision_questions: an answer outlives the revision that
-- asked, so a later push re-declaring the same key finds the answer waiting.
CREATE TABLE document_answers (
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    selected    TEXT NOT NULL,
    other_text  TEXT,
    notes       TEXT,
    answered_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (document_id, key)
);
