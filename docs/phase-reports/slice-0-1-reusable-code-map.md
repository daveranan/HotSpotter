# Slice 0-1 reusable-code map

**Scope:** Repository evidence for the template-and-Blender program; no schema or renderer change is made by this report.

| Concern | Reusable location | Reuse boundary |
| --- | --- | --- |
| Generic packer | `crates/geometry/src/layout.rs` | Retain as the Custom Atlas solver; template slots must bypass its placement decisions. |
| Region persistence | `crates/project-store/src/lib.rs`, `crates/domain/src/lib.rs` | Preserve existing region IDs, colors, locks, and layout persistence while Slice 2 adds template snapshots. |
| Region ID generation | `crates/geometry/src/layout.rs` | Existing deterministic ID/color handling is a basis for persistent template IDs; new template colors must be collision-validated. |
| Layout UI | `apps/desktop/src/features/layout/` | Reuse the advanced layout surface for Custom Atlas; basic template controls must not expose freeform topology editing. |
| Renderer hooks | `crates/render-core/src/lib.rs` | Reuse authoritative CPU rendering and cancellation/progress boundary for template compilation; add profiles and IDs in later slices. |
| Export hooks | `crates/export/src/lib.rs` | Reuse snapshot validation and atomic export boundary for versioned `.hottrim` revision publishing. |

The map is constrained by the consolidated plan: Rust remains authoritative for template geometry; TypeScript renders and edits only through domain-backed contracts.

