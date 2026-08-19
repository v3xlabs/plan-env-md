-- Both the store and the queue: the job and its result share a primary key, so
-- a second table would only need joining back.
CREATE TABLE revision_previews (
    revision_id  INTEGER NOT NULL REFERENCES revisions(id) ON DELETE CASCADE,
    scheme       TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending',
    attempts     INTEGER NOT NULL DEFAULT 0,
    image        BLOB,
    content_type TEXT,
    width        INTEGER,
    height       INTEGER,
    error        TEXT,
    updated_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (revision_id, scheme),
    CHECK (scheme IN ('light', 'dark')),
    CHECK (status IN ('pending', 'running', 'ready', 'failed'))
);
CREATE INDEX idx_revision_previews_queue ON revision_previews(status, revision_id);

-- queue the latest revision of every existing document, in both schemes, so the
-- list has thumbnails on the first boot after this migration. Older revisions
-- stay unqueued: they are reachable from the detail page and can be rendered on
-- demand later if that turns out to matter.
INSERT INTO revision_previews (revision_id, scheme)
SELECT r.id, s.scheme
FROM revisions r
JOIN (SELECT document_id, MAX(revision) AS revision FROM revisions GROUP BY document_id) latest
  ON latest.document_id = r.document_id AND latest.revision = r.revision
CROSS JOIN (SELECT 'light' AS scheme UNION ALL SELECT 'dark' AS scheme) s;
