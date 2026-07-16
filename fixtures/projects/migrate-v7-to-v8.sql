ALTER TABLE layouts ADD COLUMN layout_kind TEXT NOT NULL DEFAULT 'custom_atlas'
    CHECK(layout_kind IN ('template', 'custom_atlas'));
ALTER TABLE layouts ADD COLUMN template_json TEXT
    CHECK(template_json IS NULL OR (json_valid(template_json) AND length(template_json) <= 1048576));