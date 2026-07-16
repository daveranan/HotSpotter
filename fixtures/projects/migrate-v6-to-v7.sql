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
