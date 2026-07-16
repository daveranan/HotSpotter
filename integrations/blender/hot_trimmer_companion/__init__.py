bl_info = {"name": "Hot Trimmer Companion", "author": "Hot Trimmer", "version": (0, 1, 0), "blender": (4, 0, 0), "category": "Material"}

from .operators import HOTTRIM_OT_import_package, HOTTRIM_OT_fit_selected

_CLASSES = (HOTTRIM_OT_import_package, HOTTRIM_OT_fit_selected)

def register():
    for cls in _CLASSES:
        __import__("bpy").utils.register_class(cls)

def unregister():
    for cls in reversed(_CLASSES):
        __import__("bpy").utils.unregister_class(cls)