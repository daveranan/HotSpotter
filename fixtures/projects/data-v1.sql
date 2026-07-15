INSERT INTO project (id, name, created_unix, modified_unix)
VALUES ('00000000-0000-4000-8000-000000000001', 'Version One', 1, 1);
INSERT INTO sources (
    id, channel, ownership, external_path, sha256, width, height, format, color_type,
    has_alpha, exif_orientation, has_icc_profile, encoded_bytes, owned_bytes
) VALUES (
    '00000000-0000-4000-8000-000000000002', 'base_color', 'owned_copy', NULL,
    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 1, 1, 'PNG', 'Rgba8',
    1, 1, 0, 1, X'00'
);
