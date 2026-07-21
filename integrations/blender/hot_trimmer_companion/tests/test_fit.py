import json
import math
import tempfile
import unittest
from pathlib import Path

from hot_trimmer_companion.fit import IslandDescriptor, choose_slot, points_inside_slot, transform_uvs
from hot_trimmer_companion.manifest import load_manifest, slots


FIXTURES = Path(__file__).parent / "fixtures"


class ManifestAndMatchingTests(unittest.TestCase):
    def setUp(self):
        self.manifest = load_manifest(FIXTURES / "behavioral.hottrim.json")

    def test_projects_complete_slot_contract_without_using_id_color(self):
        rectangular, _, radial = slots(self.manifest)
        self.assertEqual(rectangular.slot_id, "rect_wide")
        self.assertEqual(rectangular.region_id, "fixture-v1:rect_wide")
        self.assertEqual(rectangular.fit_axis, "automatic")
        self.assertTrue(rectangular.keep_proportion)
        self.assertEqual(rectangular.allowed_rotations, (0, 180))
        self.assertTrue(rectangular.mirror_allowed)
        self.assertEqual(rectangular.classification_tags, ("HOTSPOT", "RECTANGULAR"))
        self.assertEqual(rectangular.world_size_meters, (2.0, 1.0))
        self.assertEqual(rectangular.variation_group, "rectangles")
        self.assertTrue(rectangular.enabled)
        self.assertEqual(radial.uv_fit_kind, "radial")
        self.assertIsNotNone(radial.radial_parameters)
        self.assertEqual(radial.region_id_color, rectangular.region_id_color)

    def test_rejects_malformed_bounds_before_projection(self):
        data = json.loads((FIXTURES / "behavioral.hottrim.json").read_text(encoding="utf-8"))
        data["slots"][0]["normalizedHotspotRect"]["x"] = 0.9
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "bad.hottrim.json"
            path.write_text(json.dumps(data), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "normalized"):
                load_manifest(path)

    def test_exported_package_directory_resolves_canonical_manifest(self):
        with tempfile.TemporaryDirectory(suffix=".hottrim") as directory:
            package = Path(directory)
            manifest_path = package / "manifest.hottrim.json"
            manifest_path.write_text((FIXTURES / "behavioral.hottrim.json").read_text(encoding="utf-8"), encoding="utf-8")
            manifest = load_manifest(package)
            self.assertEqual(manifest["_manifest_path"], manifest_path.resolve())
            self.assertEqual(manifest["_package_path"], package.resolve())

    def test_parent_with_one_exported_package_resolves_without_recursive_guessing(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            package = parent / "Fixture.hottrim"
            package.mkdir()
            (package / "manifest.hottrim.json").write_text((FIXTURES / "behavioral.hottrim.json").read_text(encoding="utf-8"), encoding="utf-8")
            self.assertEqual(load_manifest(parent)["_package_path"], package.resolve())

    def test_rectangular_matching_is_aspect_first_and_slot_id_stable(self):
        descriptor = IslandDescriptor((0.0, 0.0, 2.0, 1.0), 2.0, 2.0, 2.0, "U", True, 0.4)
        ordered = choose_slot(descriptor, reversed(slots(self.manifest)))
        self.assertEqual(ordered.slot.slot_id, "rect_wide")
        self.assertEqual(ordered.rotation, 0)
        repeated = choose_slot(descriptor, slots(self.manifest))
        self.assertEqual((ordered.slot.slot_id, ordered.rotation), (repeated.slot.slot_id, repeated.rotation))

    def test_rectangular_transform_is_uniform_and_bounded(self):
        descriptor = IslandDescriptor((0.0, 0.0, 2.0, 1.0), 2.0, 2.0, 2.0, "U", True, 0.4)
        match = choose_slot(descriptor, slots(self.manifest))
        original = ((0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (0.0, 1.0))
        fitted = transform_uvs(original, match)
        self.assertTrue(points_inside_slot(fitted, match.slot))
        horizontal = math.dist(fitted[0], fitted[1]) / math.dist(original[0], original[1])
        vertical = math.dist(fitted[1], fitted[2]) / math.dist(original[1], original[2])
        self.assertAlmostEqual(horizontal, vertical, places=12)

    def test_auto_radial_requires_strong_evidence_and_manual_override_wins(self):
        radial = IslandDescriptor((0.0, 0.0, 1.0, 1.0), 1.0, math.pi / 4, math.pi, "U", True, 0.97)
        self.assertEqual(choose_slot(radial, slots(self.manifest)).slot.slot_id, "radial_disc")
        square = IslandDescriptor((0.0, 0.0, 1.0, 1.0), 1.0, 1.0, 1.0, "U", True, 0.78)
        self.assertTrue(choose_slot(square, slots(self.manifest), "RECTANGULAR").slot.slot_id.startswith("rect_"))
        with self.assertRaisesRegex(ValueError, "unsupported radial topology"):
            choose_slot(square, slots(self.manifest), "RADIAL")


if __name__ == "__main__":
    unittest.main()
