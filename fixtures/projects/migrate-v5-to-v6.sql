PRAGMA defer_foreign_keys = ON;

ALTER TABLE patches RENAME TO patches_v5;
ALTER TABLE sources RENAME TO sources_v5;

CREATE TABLE source_sets (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 255),
    ordinal INTEGER NOT NULL UNIQUE CHECK(ordinal >= 0)
);

INSERT INTO source_sets (id, name, ordinal)
SELECT id, 'Material 1', 0 FROM project;

CREATE TABLE sources (
    id TEXT PRIMARY KEY NOT NULL,
    source_set_id TEXT NOT NULL,
    channel TEXT NOT NULL CHECK(channel IN (
        'base_color', 'normal', 'height', 'roughness', 'metallic', 'ambient_occlusion',
        'specular', 'opacity', 'edge_mask', 'material_id'
    )),
    ownership TEXT NOT NULL CHECK(ownership IN ('owned_copy', 'verified_external_reference')),
    external_path TEXT,
    sha256 TEXT NOT NULL CHECK(length(sha256) = 64),
    width INTEGER NOT NULL CHECK(width > 0),
    height INTEGER NOT NULL CHECK(height > 0),
    format TEXT NOT NULL CHECK(format IN ('PNG', 'JPEG', 'TIFF')),
    color_type TEXT NOT NULL,
    has_alpha INTEGER NOT NULL,
    exif_orientation INTEGER NOT NULL CHECK(exif_orientation BETWEEN 1 AND 8),
    has_icc_profile INTEGER NOT NULL,
    encoded_bytes INTEGER NOT NULL CHECK(encoded_bytes > 0),
    owned_bytes BLOB,
    origin_path TEXT NOT NULL,
    UNIQUE(source_set_id, channel),
    FOREIGN KEY(source_set_id) REFERENCES source_sets(id) ON DELETE CASCADE,
    CHECK(
        (ownership = 'owned_copy' AND owned_bytes IS NOT NULL AND external_path IS NULL) OR
        (ownership = 'verified_external_reference' AND owned_bytes IS NULL AND external_path IS NOT NULL)
    )
);

INSERT INTO sources (
    id, source_set_id, channel, ownership, external_path, sha256, width, height, format,
    color_type, has_alpha, exif_orientation, has_icc_profile, encoded_bytes, owned_bytes,
    origin_path
)
SELECT
    id, (SELECT id FROM project LIMIT 1), channel, ownership, external_path, sha256, width,
    height, format, color_type, has_alpha, exif_orientation, has_icc_profile, encoded_bytes,
    owned_bytes, origin_path
FROM sources_v5;

CREATE TABLE patches (
    id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL UNIQUE CHECK(ordinal >= 0),
    patch_json TEXT NOT NULL CHECK(json_valid(patch_json) AND length(patch_json) <= 65536),
    FOREIGN KEY(source_id) REFERENCES sources(id) DEFERRABLE INITIALLY DEFERRED
);

INSERT INTO patches SELECT * FROM patches_v5;

DROP TABLE patches_v5;
DROP TABLE sources_v5;
