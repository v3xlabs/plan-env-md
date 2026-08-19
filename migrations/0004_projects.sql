-- A project was a bare column, which grouped and filtered fine. Favicons give
-- it state of its own, so it becomes a row. `documents.project` still holds the
-- slug: the slug is the identity, it is what the URL and the filter use, and a
-- surrogate key would mean rewriting every query that already works.
CREATE TABLE projects (
    id                 INTEGER PRIMARY KEY,
    owner_id           INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    slug               TEXT NOT NULL,
    favicon_light      BLOB,
    favicon_light_type TEXT,
    favicon_dark       BLOB,
    favicon_dark_type  TEXT,
    created_at         TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (owner_id, slug)
);

-- every project a document already names becomes a row, so the projects page
-- is complete on the first boot after this migration
INSERT INTO projects (owner_id, slug)
SELECT DISTINCT owner_id, project FROM documents WHERE project IS NOT NULL;
