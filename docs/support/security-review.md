# Phase 0/1 Security, Dependency, and Parser Review

Date: 2026-07-15

## Dependency and License Review

- JavaScript dependencies are lockfile-pinned and `npm audit --audit-level=high` runs in CI.
- Rust dependencies are lockfile-pinned and the RustSec advisory database runs in CI through
  `rustsec/audit-check`.
- `cargo metadata --format-version 1 --locked` was reviewed across the resolved graph. Dependencies use MIT,
  Apache-2.0, BSD, ISC, Zlib, Unicode, CC0, Unlicense, or MPL-2.0 expressions; `r-efi` offers MIT/Apache choices.
- MPL-2.0 crates (`cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`, and `selectors`) use file-level
  copyleft. Their upstream notices and modified-source obligations must be included in release qualification.
- Workspace crates use `LicenseRef-Proprietary`. No dependency has a GPL-only, AGPL-only, or LGPL-only
  expression in the resolved graph.

## Parser Threat Model

Untrusted inputs in Phase 1 are PNG, JPEG, TIFF, EXIF, ICC profiles, SQLite project files, JSON recent-project
metadata, drag/drop paths, and Tauri IPC JSON.

Controls:

- Encoded input is limited to 512 MiB, dimensions to 16,384 pixels per edge, and conservative decoded
  allocation to 1 GiB before full decode.
- Supported image codecs are explicitly allowlisted. Malformed/truncated inputs, dimension bombs, encoded and
  decoded bounds, EXIF rotation, ICC conversion, alpha, and cooperative cancellation have regression tests.
- ICC transforms apply only under explicit Base Color policy; every other slot is linear data.
- IPC protocol versions and Windows path length are validated. Thumbnail IPC is limited to three bounded mips;
  authoritative source buffers never cross JSON IPC.
- SQLite runs bundled, with schema checks, transactional migration, integrity checks, one-writer locks, and
  read-only inspection before recovery publication.
- External references are re-hashed before use. Owned source bytes are immutable inside the project.
- No runtime network client, archive extractor, scripting engine, or general plugin loader is present.

Residual risks:

- A codec call cannot be interrupted mid-call after bounded allocation checks; cancellation occurs between
  parser, decode, profile, mip, and persistence units.
- Parser dependencies remain subject to upstream advisories; RustSec and npm audit are release gates, not a
  substitute for updating affected libraries.
- Low-disk and permission transitions can prevent recovery refresh. The authoritative command remains dirty and
  emits a warning so users can save explicitly.
