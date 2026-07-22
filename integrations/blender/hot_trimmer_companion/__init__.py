bl_info = {
    "name": "Hot Trimmer Companion",
    "author": "Hot Trimmer",
    "version": (0, 3, 14),
    "blender": (4, 0, 0),
    "category": "Material",
}

try:
    import bpy
except ModuleNotFoundError:  # Pure matching/manifest tests intentionally run outside Blender.
    bpy = None

if bpy is not None:
    from bpy.props import BoolProperty, EnumProperty
    from . import operators
    from .operators import (
        CLASSIFICATION_ITEMS,
        HOTTRIM_OT_fit_selected,
        HOTTRIM_OT_import_package,
        HOTTRIM_PT_panel,
    )
    _AUTO_UPDATE_HANDLER = getattr(operators, "hottrimmer_auto_update_handler", None)
    _AUTO_UPDATE_START = getattr(operators, "start_auto_updates", None)
    _AUTO_UPDATE_STOP = getattr(operators, "stop_auto_updates", None)
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
    bpy.types.Scene.hottrimmer_live_update = BoolProperty(
        name="Live Mesh Updates",
        description="Automatically re-hotspot an assigned mesh after topology or vertex edits",
        default=True,
    )
    if _AUTO_UPDATE_HANDLER is not None and _AUTO_UPDATE_HANDLER not in bpy.app.handlers.depsgraph_update_post:
        bpy.app.handlers.depsgraph_update_post.append(_AUTO_UPDATE_HANDLER)
    if _AUTO_UPDATE_START is not None:
        _AUTO_UPDATE_START()


def unregister():
    if bpy is None:
        return
    if hasattr(bpy.types.Scene, "hottrimmer_classification"):
        del bpy.types.Scene.hottrimmer_classification
    if hasattr(bpy.types.Scene, "hottrimmer_live_update"):
        del bpy.types.Scene.hottrimmer_live_update
    if _AUTO_UPDATE_STOP is not None:
        _AUTO_UPDATE_STOP()
    if _AUTO_UPDATE_HANDLER is not None and _AUTO_UPDATE_HANDLER in bpy.app.handlers.depsgraph_update_post:
        bpy.app.handlers.depsgraph_update_post.remove(_AUTO_UPDATE_HANDLER)
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
