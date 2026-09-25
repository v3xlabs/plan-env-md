-- The drawn placeholder is captured by the same job as the screenshot, from the
-- same settled page, so it shares the row and its lifecycle. It is SVG text of
-- a few kilobytes and stays inline; only image moves to the bucket.
ALTER TABLE revision_previews ADD COLUMN placeholder TEXT;

-- Render the latest revision of every document again so it gains a placeholder.
-- The stored screenshot stays in the row and keeps being served until the new
-- capture replaces it. Older revisions stay as they are, as in 0005.
UPDATE revision_previews
SET status = 'pending', attempts = 0, error = NULL
WHERE status = 'ready'
  AND revision_id IN (
    SELECT r.id
    FROM revisions r
    JOIN (SELECT document_id, MAX(revision) AS revision FROM revisions GROUP BY document_id) latest
      ON latest.document_id = r.document_id AND latest.revision = r.revision
  );
