CREATE TABLE project (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 255),
    created_unix INTEGER NOT NULL,
    modified_unix INTEGER NOT NULL
);
CREATE TABLE sources (
    id TEXT PRIMARY KEY NOT NULL,
    channel TEXT NOT NULL UNIQUE CHECK(channel IN (
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
    origin_path TEXT NOT NULL DEFAULT '',
    CHECK(
        (ownership = 'owned_copy' AND owned_bytes IS NOT NULL AND external_path IS NULL) OR
        (ownership = 'verified_external_reference' AND owned_bytes IS NULL AND external_path IS NOT NULL)
    )
);
CREATE TABLE autosave_journal (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_unix INTEGER NOT NULL,
    operation TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK(json_valid(payload_json))
);
