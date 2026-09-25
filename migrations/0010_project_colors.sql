-- The mark a project shows when it has no icon. NULL means the client derives
-- one from the slug, so every project has a colour without anyone picking.
ALTER TABLE projects ADD COLUMN color TEXT
    CHECK (color IN ('red', 'orange', 'amber', 'emerald', 'teal', 'sky', 'indigo', 'pink'));
