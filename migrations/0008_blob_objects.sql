-- A body lives inline or in the bucket, never both and never neither. SQLite
-- cannot relax the NOT NULL on content with ALTER TABLE, so the table is
-- rebuilt, the same way migration 0006 rebuilt it to become a file set.
CREATE TABLE revision_files_new (
    id           INTEGER PRIMARY KEY,
    revision_id  INTEGER NOT NULL REFERENCES revisions(id) ON DELETE CASCADE,
    path         TEXT NOT NULL,
    content      BLOB,
    object_key   TEXT,
    content_type TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL,
    UNIQUE (revision_id, path),
    CHECK ((content IS NULL) != (object_key IS NULL))
);

INSERT INTO revision_files_new (id, revision_id, path, content, content_type, size_bytes)
SELECT id, revision_id, path, content, content_type, size_bytes FROM revision_files;

DROP TABLE revision_files;
ALTER TABLE revision_files_new RENAME TO revision_files;
CREATE INDEX idx_revision_files_revision ON revision_files(revision_id);

-- Previews carry a key the same way. image stays nullable because a queued or
-- failed row has no body yet, so the pair cannot use revision_files' CHECK.
ALTER TABLE revision_previews ADD COLUMN object_key TEXT;
