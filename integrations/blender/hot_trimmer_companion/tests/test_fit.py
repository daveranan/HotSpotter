import json
import unittest
from pathlib import Path
from hot_trimmer_companion.fit import fit_values
from hot_trimmer_companion.manifest import Slot, selected_fit_kind

FIXTURES = Path(__file__).parent / "fixtures"

def fixture_slot(name):
    data = json.loads((FIXTURES / name).read_text(encoding="utf-8"))
    record = data["slots"][0]
    return Slot(record["slotId"], record["uvFit"]["kind"], record["normalizedHotspotRect"], tuple(record["regionIdColor"]))

class FitFixtureTests(unittest.TestCase):
    def test_rectangular_fixture_uses_manifest_rectangle(self):
        slot = fixture_slot("rectangular.hottrim.json")
        self.assertEqual(fit_values(slot), ((0.1, 0.2), (0.5, 0.2), (0.5, 0.4), (0.1, 0.4)))
    def test_radial_fixture_never_uses_id_color_for_classification(self):
        slot = fixture_slot("radial.hottrim.json")
        self.assertEqual(selected_fit_kind(slot, "AUTO"), "radial")
        self.assertEqual(fit_values(slot), ((0.5, 0.5), 0.25))