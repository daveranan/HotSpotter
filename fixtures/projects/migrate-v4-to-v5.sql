CREATE TABLE patches (
    id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL UNIQUE CHECK(ordinal >= 0),
    patch_json TEXT NOT NULL CHECK(json_valid(patch_json) AND length(patch_json) <= 65536),
    FOREIGN KEY(source_id) REFERENCES sources(id) DEFERRABLE INITIALLY DEFERRED
);
