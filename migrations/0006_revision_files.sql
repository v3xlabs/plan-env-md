-- A revision held one HTML blob. It becomes a file set so a document can ship
-- the stylesheet, script and images it references.
CREATE TABLE revision_files (
    id           INTEGER PRIMARY KEY,
    revision_id  INTEGER NOT NULL REFERENCES revisions(id) ON DELETE CASCADE,
    path         TEXT NOT NULL,
    content      BLOB NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes   INTEGER NOT NULL,
    UNIQUE (revision_id, path)
);
CREATE INDEX idx_revision_files_revision ON revision_files(revision_id);

INSERT INTO revision_files (revision_id, path, content, content_type, size_bytes)
SELECT id, 'index.html', html, 'text/html; charset=utf-8', size_bytes
FROM revisions;

-- revisions.size_bytes keeps its name and changes meaning from "size of the
-- HTML" to "total across the revision's files". For every existing row those
-- are the same number, so no arithmetic is needed here.
ALTER TABLE revisions DROP COLUMN html;
