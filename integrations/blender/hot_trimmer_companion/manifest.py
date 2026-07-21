"""Validated reader for the exported Hot Trimmer package contract.

The manifest is the sole atlas authority.  In particular, ``regionIdColor`` is
retained for diagnostics but never participates in fitting or classification.
"""

from dataclasses import dataclass
import json
import math
from pathlib import Path


SUPPORTED_SCHEMA_VERSION = 1
MANIFEST_FILE_NAME = "manifest.hottrim.json"
SUPPORTED_KINDS = frozenset(("rectangular", "radial"))
SUPPORTED_FIT_AXES = frozenset(("automatic", "none"))
SUPPORTED_ROTATIONS = frozenset((0, 90, 180, 270))


@dataclass(frozen=True)
class Slot:
    slot_id: str
    region_id: str
    role: str
    normalized_hotspot_rect: dict
    uv_fit_kind: str
    fit_axis: str
    keep_proportion: bool
    allowed_rotations: tuple
    mirror_allowed: bool
    classification_tags: tuple
    world_size_meters: tuple
    variation_group: str
    enabled: bool
    radial_parameters: dict | None
    region_id_color: tuple | None = None

    @property
    def is_radial(self):
        return self.uv_fit_kind == "radial"


def _required(mapping, key, description):
    if key not in mapping:
        raise ValueError(f"{description} is missing required field {key}")
    return mapping[key]


def _nonempty_string(value, description):
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{description} must be a non-empty string")
    return value


def _finite_number(value, description):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError(f"{description} must be a finite number")
    return float(value)


def _validated_rect(value, description):
    if not isinstance(value, dict):
        raise ValueError(f"{description} must be an object")
    rect = {key: _finite_number(_required(value, key, description), f"{description}.{key}") for key in ("x", "y", "width", "height")}
    if rect["width"] <= 0.0 or rect["height"] <= 0.0:
        raise ValueError(f"{description} must have positive width and height")
    if rect["x"] < 0.0 or rect["y"] < 0.0 or rect["x"] + rect["width"] > 1.0 or rect["y"] + rect["height"] > 1.0:
        raise ValueError(f"{description} must be within normalized [0, 1] bounds")
    return rect


def _validated_slot(record, index):
    description = f"slot[{index}]"
    if not isinstance(record, dict):
        raise ValueError(f"{description} must be an object")
    uv_fit = _required(record, "uvFit", description)
    if not isinstance(uv_fit, dict):
        raise ValueError(f"{description}.uvFit must be an object")
    kind = _nonempty_string(_required(uv_fit, "kind", f"{description}.uvFit"), f"{description}.uvFit.kind").lower()
    if kind not in SUPPORTED_KINDS:
        raise ValueError(f"{description}.uvFit.kind is unsupported: {kind}")
    fit_axis = _nonempty_string(_required(uv_fit, "fitAxis", f"{description}.uvFit"), f"{description}.uvFit.fitAxis").lower()
    if fit_axis not in SUPPORTED_FIT_AXES:
        raise ValueError(f"{description}.uvFit.fitAxis is unsupported: {fit_axis}")
    keep_proportion = _required(uv_fit, "keepProportion", f"{description}.uvFit")
    mirror_allowed = _required(uv_fit, "mirrorAllowed", f"{description}.uvFit")
    enabled = _required(record, "enabled", description)
    if not all(isinstance(value, bool) for value in (keep_proportion, mirror_allowed, enabled)):
        raise ValueError(f"{description} boolean fields are malformed")
    rotations = _required(uv_fit, "allowedRotations", f"{description}.uvFit")
    if not isinstance(rotations, list) or not rotations or any(isinstance(value, bool) or not isinstance(value, int) or value not in SUPPORTED_ROTATIONS for value in rotations):
        raise ValueError(f"{description}.uvFit.allowedRotations must contain supported quarter turns")
    tags = _required(uv_fit, "classificationTags", f"{description}.uvFit")
    if not isinstance(tags, list) or any(not isinstance(tag, str) for tag in tags):
        raise ValueError(f"{description}.uvFit.classificationTags must be a string array")
    world_size = _required(record, "worldSizeMeters", description)
    if not isinstance(world_size, list) or len(world_size) != 2:
        raise ValueError(f"{description}.worldSizeMeters must contain width and height")
    world_size = tuple(_finite_number(value, f"{description}.worldSizeMeters") for value in world_size)
    if world_size[0] <= 0.0 or world_size[1] <= 0.0:
        raise ValueError(f"{description}.worldSizeMeters must be positive")
    radial_parameters = record.get("radialParameters")
    if radial_parameters is not None and not isinstance(radial_parameters, dict):
        raise ValueError(f"{description}.radialParameters must be an object when present")
    color = record.get("regionIdColor")
    if color is not None:
        if not isinstance(color, list) or len(color) != 3 or any(isinstance(channel, bool) or not isinstance(channel, int) or channel < 0 or channel > 255 for channel in color):
            raise ValueError(f"{description}.regionIdColor must be an RGB byte triplet")
        color = tuple(color)
    return Slot(
        slot_id=_nonempty_string(_required(record, "slotId", description), f"{description}.slotId"),
        region_id=_nonempty_string(_required(record, "regionId", description), f"{description}.regionId"),
        role=_nonempty_string(_required(record, "role", description), f"{description}.role").lower(),
        normalized_hotspot_rect=_validated_rect(_required(record, "normalizedHotspotRect", description), f"{description}.normalizedHotspotRect"),
        uv_fit_kind=kind,
        fit_axis=fit_axis,
        keep_proportion=keep_proportion,
        allowed_rotations=tuple(sorted(set(rotations))),
        mirror_allowed=mirror_allowed,
        classification_tags=tuple(tags),
        world_size_meters=world_size,
        variation_group=_nonempty_string(_required(record, "variationGroup", description), f"{description}.variationGroup"),
        enabled=enabled,
        radial_parameters=radial_parameters,
        region_id_color=color,
    )


def _validate_map(record, key, package_path):
    description = f"map[{key}]"
    if not isinstance(record, dict):
        raise ValueError(f"{description} must be an object")
    _nonempty_string(_required(record, "role", description), f"{description}.role")
    relative = _nonempty_string(_required(record, "relativePath", description), f"{description}.relativePath")
    candidate = (package_path / relative).resolve()
    try:
        candidate.relative_to(package_path.resolve())
    except ValueError as error:
        raise ValueError(f"{description}.relativePath escapes the package") from error
    if not candidate.is_file():
        raise ValueError(f"{description} image is missing: {relative}")


def validate_manifest(data, manifest_path):
    if not isinstance(data, dict):
        raise ValueError("Hot Trimmer manifest root must be an object")
    if data.get("schemaVersion") != SUPPORTED_SCHEMA_VERSION:
        raise ValueError("unsupported Hot Trimmer manifest schema")
    for key in ("projectId", "materialId", "materialName", "templateId", "templateVersion", "compatibilityKey", "templateSnapshotHash"):
        _nonempty_string(_required(data, key, "manifest"), f"manifest.{key}")
    revision = _required(data, "materialRevision", "manifest")
    if isinstance(revision, bool) or not isinstance(revision, int) or revision < 0:
        raise ValueError("manifest.materialRevision must be a non-negative integer")
    output_size = _required(data, "outputSize", "manifest")
    if not isinstance(output_size, list) or len(output_size) != 2 or any(isinstance(value, bool) or not isinstance(value, int) or value <= 0 for value in output_size):
        raise ValueError("manifest.outputSize must contain two positive integers")
    orientation = _nonempty_string(_required(data, "normalOrientation", "manifest"), "manifest.normalOrientation").lower()
    if orientation not in ("opengl", "directx"):
        raise ValueError("manifest.normalOrientation must be OpenGL or DirectX")
    maps = _required(data, "maps", "manifest")
    if not isinstance(maps, dict):
        raise ValueError("manifest.maps must be an object")
    package_path = manifest_path.parent
    for key, record in maps.items():
        _validate_map(record, key, package_path)
    records = _required(data, "slots", "manifest")
    if not isinstance(records, list) or not records:
        raise ValueError("manifest.slots must contain at least one slot")
    projected = tuple(_validated_slot(record, index) for index, record in enumerate(records))
    identifiers = [slot.slot_id for slot in projected]
    if len(identifiers) != len(set(identifiers)):
        raise ValueError("manifest slotId values must be unique")
    data["_package_path"] = package_path
    data["_manifest_path"] = manifest_path
    data["_slots"] = projected
    return data


def resolve_manifest_path(path):
    """Accept either an exported package directory or its manifest file."""
    selected_path = Path(path).expanduser().resolve()
    if selected_path.is_file():
        return selected_path
    if selected_path.is_dir():
        canonical = selected_path / MANIFEST_FILE_NAME
        if canonical.is_file():
            return canonical
        manifests = sorted(candidate for candidate in selected_path.glob("*.hottrim.json") if candidate.is_file())
        if len(manifests) == 1:
            return manifests[0]
        package_manifests = sorted(
            candidate / MANIFEST_FILE_NAME
            for candidate in selected_path.iterdir()
            if candidate.is_dir() and candidate.name.lower().endswith(".hottrim") and (candidate / MANIFEST_FILE_NAME).is_file()
        )
        if len(package_manifests) == 1:
            return package_manifests[0]
        if len(manifests) + len(package_manifests) > 1:
            raise ValueError("multiple Hot Trimmer packages were found; select the intended .hottrim folder or manifest.hottrim.json")
        raise ValueError(f"Hot Trimmer package does not contain {MANIFEST_FILE_NAME}: {selected_path}")
    raise ValueError(f"Hot Trimmer package or manifest does not exist: {selected_path}")


def load_manifest(path):
    manifest_path = resolve_manifest_path(path)
    try:
        data = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read Hot Trimmer manifest: {error}") from error
    return validate_manifest(data, manifest_path)


def slots(manifest):
    return manifest["_slots"]


def selected_fit_kind(slot, override):
    """Apply the explicit UI override; Auto preserves manifest topology metadata."""
    return slot.uv_fit_kind if override == "AUTO" else override.lower()
