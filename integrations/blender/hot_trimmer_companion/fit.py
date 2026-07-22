"""Deterministic hotspot matching and proportion-preserving UV transforms."""

from dataclasses import dataclass
import math


EPSILON = 1.0e-9


@dataclass(frozen=True)
class IslandDescriptor:
    uv_bounds: tuple
    uv_aspect: float
    uv_area: float
    world_area: float
    long_axis_orientation: str
    boundary_closed: bool
    circularity: float
    existing_slot_id: str | None = None
    existing_compatibility_key: str | None = None

    @property
    def strongly_radial(self):
        return self.boundary_closed and 0.8 <= self.uv_aspect <= 1.25 and self.circularity >= 0.85


@dataclass(frozen=True)
class Match:
    slot: object
    rotation: int
    mirror: bool
    classification: str


def rectangular_corners(slot):
    rect = slot.normalized_hotspot_rect
    x, y, width, height = rect["x"], rect["y"], rect["width"], rect["height"]
    return ((x, y), (x + width, y), (x + width, y + height), (x, y + height))


def radial_fit(slot):
    rect = slot.normalized_hotspot_rect
    return ((rect["x"] + rect["width"] / 2, rect["y"] + rect["height"] / 2), min(rect["width"], rect["height"]) / 2)


def fit_values(slot, override="AUTO"):
    kind = slot.uv_fit_kind if override == "AUTO" else override.lower()
    return radial_fit(slot) if kind == "radial" else rectangular_corners(slot)


def polygon_signed_area(points):
    return 0.5 * sum(points[index][0] * points[(index + 1) % len(points)][1] - points[(index + 1) % len(points)][0] * points[index][1] for index in range(len(points)))


def bounds(points):
    min_u = min(point[0] for point in points)
    min_v = min(point[1] for point in points)
    max_u = max(point[0] for point in points)
    max_v = max(point[1] for point in points)
    return (min_u, min_v, max_u, max_v)


def circularity_estimate(boundary_points, area):
    if len(boundary_points) < 3 or area <= EPSILON:
        return 0.0
    center = (sum(point[0] for point in boundary_points) / len(boundary_points), sum(point[1] for point in boundary_points) / len(boundary_points))
    radii = [math.hypot(point[0] - center[0], point[1] - center[1]) for point in boundary_points]
    mean_radius = sum(radii) / len(radii)
    if mean_radius <= EPSILON:
        return 0.0
    radial_score = max(0.0, 1.0 - math.sqrt(sum((radius - mean_radius) ** 2 for radius in radii) / len(radii)) / mean_radius)
    ordered = sorted(boundary_points, key=lambda point: math.atan2(point[1] - center[1], point[0] - center[0]))
    perimeter = sum(math.dist(ordered[index], ordered[(index + 1) % len(ordered)]) for index in range(len(ordered)))
    compactness = min(1.0, 4.0 * math.pi * area / max(perimeter * perimeter, EPSILON))
    return max(0.0, min(1.0, radial_score * compactness))


def classify_island(descriptor, override):
    if override == "RADIAL":
        if not descriptor.strongly_radial:
            raise ValueError("unsupported radial topology")
        return "radial"
    if override == "RECTANGULAR":
        return "rectangular"
    return "radial" if descriptor.strongly_radial else "rectangular"


def island_behavior_role(descriptor, classification):
    if classification == "radial":
        return "radial"
    if descriptor.uv_aspect >= 4.0:
        return "horizontal_strip"
    if descriptor.uv_aspect <= 0.25:
        return "vertical_strip"
    return "panel"


def _slot_behavior_role(slot):
    if slot.behavior_role:
        return slot.behavior_role
    tags = {tag.lower() for tag in slot.classification_tags}
    for role in ("horizontal_strip", "vertical_strip", "unique", "panel", "radial"):
        if role in tags:
            return role
    normalized_role = slot.role.replace("_", "").lower()
    if normalized_role == "repeatingstrip":
        return "horizontal_strip" if slot.normalized_hotspot_rect["width"] >= slot.normalized_hotspot_rect["height"] else "vertical_strip"
    if normalized_role in {"uniquedetail", "trimcap"}:
        return "unique"
    if normalized_role == "radial":
        return "radial"
    return "panel"


def _role_compatible(slot, classification, island_role):
    slot_role = _slot_behavior_role(slot)
    if classification == "radial":
        return slot_role == "radial"
    if island_role in {"horizontal_strip", "vertical_strip"}:
        return slot_role in {"horizontal_strip", "vertical_strip"}
    return slot_role in {"panel", "unique"}


def choose_slot(descriptor, available_slots, override="AUTO", requested_slot_id="", variation_index=0, distribute=False):
    classification = classify_island(descriptor, override)
    island_role = island_behavior_role(descriptor, classification)
    kind_candidates = [slot for slot in available_slots if slot.enabled and slot.uv_fit_kind == classification]
    candidates = [slot for slot in kind_candidates if _role_compatible(slot, classification, island_role)]
    # Role metadata expresses the best semantic match; it does not make an
    # otherwise valid rectangular atlas unusable.  Older/custom templates may
    # legitimately contain only panels or only strips.  Preserve the role
    # preference when possible and deterministically fall back to every slot
    # of the required fit kind when no exact role counterpart exists.
    if not candidates:
        candidates = kind_candidates
    if requested_slot_id:
        candidates = [slot for slot in candidates if slot.slot_id == requested_slot_id]
    if not candidates:
        raise ValueError(f"no enabled compatible {classification} hotspot in the current manifest")
    if descriptor.existing_slot_id:
        existing = next((slot for slot in candidates if slot.slot_id == descriptor.existing_slot_id), None)
        if existing is not None and not distribute:
            return Match(existing, existing.allowed_rotations[0], False, classification)
    if classification == "radial":
        def radial_score(slot):
            diameter = min(slot.world_size_meters)
            world_diameter = math.sqrt(max(descriptor.world_area, EPSILON) * 4.0 / math.pi)
            return (1.0 - descriptor.circularity, abs(math.log(max(world_diameter, EPSILON) / max(diameter, EPSILON))), slot.slot_id)
        ordered = sorted(candidates, key=radial_score)
        if distribute:
            best_cost = radial_score(ordered[0])[1]
            close = [slot for slot in ordered if radial_score(slot)[1] <= best_cost + math.log(2.0)]
            selected = close[variation_index % len(close)]
        else:
            selected = ordered[0]
        return Match(selected, selected.allowed_rotations[0], False, classification)
    scored = []
    for slot in candidates:
        rect = slot.normalized_hotspot_rect
        target_aspect = rect["width"] / rect["height"]
        target_uv_area = rect["width"] * rect["height"]
        target_world_area = slot.world_size_meters[0] * slot.world_size_meters[1]
        for rotation in slot.allowed_rotations:
            effective_aspect = target_aspect if rotation % 180 == 0 else 1.0 / target_aspect
            aspect_cost = abs(math.log(max(descriptor.uv_aspect, EPSILON) / max(effective_aspect, EPSILON)))
            uv_area_cost = abs(math.log(max(descriptor.uv_area, EPSILON) / max(target_uv_area, EPSILON)))
            world_cost = abs(math.log(max(descriptor.world_area, EPSILON) / max(target_world_area, EPSILON)))
            for mirror in ((False, True) if slot.mirror_allowed else (False,)):
                # Shape is the primary compatibility signal.  Area selects the
                # preferred entry, while click cycling may use any entry within
                # a bounded 2:1 aspect neighborhood of that best shape.
                scored.append(((aspect_cost, uv_area_cost, world_cost, slot.slot_id, rotation, mirror), slot, rotation, mirror))
    ordered = sorted(scored, key=lambda item: item[0])
    if distribute:
        best_aspect_cost = ordered[0][0][0]
        close = [item for item in ordered if item[0][0] <= best_aspect_cost + math.log(2.0)]
        _, selected, rotation, mirror = close[variation_index % len(close)]
    else:
        _, selected, rotation, mirror = ordered[0]
    return Match(selected, rotation, mirror, classification)


def transform_uvs(points, match):
    if not points:
        raise ValueError("zero-area UV island")
    center = (sum(point[0] for point in points) / len(points), sum(point[1] for point in points) / len(points))
    radians = math.radians(match.rotation)
    cosine, sine = math.cos(radians), math.sin(radians)
    transformed = []
    for u, v in points:
        x, y = u - center[0], v - center[1]
        if match.mirror:
            x = -x
        transformed.append((x * cosine - y * sine, x * sine + y * cosine))
    source = bounds(transformed)
    width, height = source[2] - source[0], source[3] - source[1]
    if width <= EPSILON or height <= EPSILON:
        raise ValueError("zero-area UV island")
    rect = match.slot.normalized_hotspot_rect
    scale = min(rect["width"] / width, rect["height"] / height)
    target_center = (rect["x"] + rect["width"] / 2.0, rect["y"] + rect["height"] / 2.0)
    source_center = ((source[0] + source[2]) / 2.0, (source[1] + source[3]) / 2.0)
    result = []
    for u, v in transformed:
        fitted = (target_center[0] + (u - source_center[0]) * scale, target_center[1] + (v - source_center[1]) * scale)
        result.append((min(rect["x"] + rect["width"], max(rect["x"], fitted[0])), min(rect["y"] + rect["height"], max(rect["y"], fitted[1]))))
    return tuple(result)


def points_inside_slot(points, slot, tolerance=1.0e-7):
    rect = slot.normalized_hotspot_rect
    return all(rect["x"] - tolerance <= point[0] <= rect["x"] + rect["width"] + tolerance and rect["y"] - tolerance <= point[1] <= rect["y"] + rect["height"] + tolerance for point in points)
