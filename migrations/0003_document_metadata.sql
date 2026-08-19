-- Project was a loose slug prefix convention. It becomes an explicit column,
-- deliberately with no backfill: `plan-env-md-mcp-plan` is genuinely ambiguous
-- between `plan`, `plan-env` and `plan-env-md`, so existing documents group
-- under "no project" until PATCH assigns one.
ALTER TABLE documents ADD COLUMN project TEXT;
CREATE INDEX idx_documents_project ON documents(owner_id, project);

CREATE TABLE document_tags (
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    tag         TEXT NOT NULL,
    PRIMARY KEY (document_id, tag)
);
CREATE INDEX idx_document_tags_tag ON document_tags(tag);
