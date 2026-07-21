bl_info = {
    "name": "Hot Trimmer Companion",
    "author": "Hot Trimmer",
    "version": (0, 2, 8),
    "blender": (4, 0, 0),
    "category": "Material",
}

try:
    import bpy
except ModuleNotFoundError:  # Pure matching/manifest tests intentionally run outside Blender.
    bpy = None

if bpy is not None:
    from bpy.props import EnumProperty
    from .operators import CLASSIFICATION_ITEMS, HOTTRIM_OT_fit_selected, HOTTRIM_OT_import_package, HOTTRIM_PT_panel
    _CLASSES = (HOTTRIM_OT_import_package, HOTTRIM_OT_fit_selected, HOTTRIM_PT_panel)


def register():
    if bpy is None:
        raise RuntimeError("Hot Trimmer companion registration requires Blender")
    for cls in _CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.Scene.hottrimmer_classification = EnumProperty(
        name="Classification",
        items=CLASSIFICATION_ITEMS,
        default="AUTO",
    )


def unregister():
    if bpy is None:
        return
    if hasattr(bpy.types.Scene, "hottrimmer_classification"):
        del bpy.types.Scene.hottrimmer_classification
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
