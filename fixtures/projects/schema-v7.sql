CREATE TABLE project (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 255),
    created_unix INTEGER NOT NULL,
    modified_unix INTEGER NOT NULL
);
CREATE TABLE source_sets (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 255),
    ordinal INTEGER NOT NULL UNIQUE CHECK(ordinal >= 0)
);
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
CREATE TABLE patches (
    id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL UNIQUE CHECK(ordinal >= 0),
    patch_json TEXT NOT NULL CHECK(json_valid(patch_json) AND length(patch_json) <= 65536),
    FOREIGN KEY(source_id) REFERENCES sources(id) DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE layouts (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
    id TEXT NOT NULL UNIQUE,
    preset TEXT NOT NULL CHECK(preset IN (
        'balanced', 'horizontal_trims', 'vertical_trims', 'modular_kit', 'atlas'
    )),
    settings_json TEXT NOT NULL CHECK(json_valid(settings_json) AND length(settings_json) <= 65536),
    items_json TEXT NOT NULL CHECK(json_valid(items_json) AND length(items_json) <= 1048576)
);
CREATE TABLE layout_regions (
    id TEXT PRIMARY KEY NOT NULL,
    layout_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL UNIQUE CHECK(ordinal >= 0),
    item_key TEXT NOT NULL UNIQUE CHECK(length(item_key) BETWEEN 1 AND 255),
    region_json TEXT NOT NULL CHECK(json_valid(region_json) AND length(region_json) <= 65536),
    FOREIGN KEY(layout_id) REFERENCES layouts(id) ON DELETE CASCADE
);
CREATE TABLE autosave_journal (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    occurred_unix INTEGER NOT NULL,
    operation TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK(json_valid(payload_json))
);
