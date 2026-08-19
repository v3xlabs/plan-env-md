-- One project, several names an agent might reach for: `openlv` and
-- `open-lavatory` are the same thing. An alias resolves to the canonical slug
-- at push time, so the grouping never forks.
CREATE TABLE project_aliases (
    owner_id   INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    alias      TEXT NOT NULL,
    project    TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (owner_id, alias)
);
CREATE INDEX idx_project_aliases_project ON project_aliases(owner_id, project);
