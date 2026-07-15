ALTER TABLE sources ADD COLUMN origin_path TEXT NOT NULL DEFAULT '';
UPDATE sources SET origin_path = COALESCE(external_path, '');
