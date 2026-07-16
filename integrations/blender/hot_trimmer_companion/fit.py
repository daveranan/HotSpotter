"""Pure fit calculations shared by the Blender operators and focused fixture tests."""
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