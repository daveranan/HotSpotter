"""Authoritative .hottrim.json reader. Radial state always comes from ``uvFit.kind``."""
from dataclasses import dataclass
import json
from pathlib import Path

@dataclass(frozen=True)
class Slot:
    slot_id: str
    uv_fit_kind: str
    normalized_hotspot_rect: dict
    region_id_color: tuple

    @property
    def is_radial(self):
        return self.uv_fit_kind == "radial"

def load_manifest(path):
    manifest_path = Path(path)
    data = json.loads(manifest_path.read_text(encoding="utf-8"))
    if data.get("schemaVersion") != 1:
        raise ValueError("unsupported Hot Trimmer manifest schema")
    data["_package_path"] = manifest_path.parent
    return data

def slots(manifest):
    return tuple(Slot(slot["slotId"], slot["uvFit"]["kind"], slot["normalizedHotspotRect"], tuple(slot["regionIdColor"])) for slot in manifest["slots"] if slot.get("enabled", True))

def selected_fit_kind(slot, override):
    """Apply the explicit UI override; Auto preserves manifest topology metadata."""
    return slot.uv_fit_kind if override == "AUTO" else override.lower()