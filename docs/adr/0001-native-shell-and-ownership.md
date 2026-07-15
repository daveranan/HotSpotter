# ADR 0001: Native Shell and Ownership Boundaries

- Status: Accepted
- Date: 2026-07-15

## Decision

Use Tauri 2 for the native shell, React and TypeScript for presentation, and Rust for domain rules,
persistence, image processing, rendering, validation, and file I/O. Large image buffers never cross JSON IPC;
the UI receives stable handles, metadata, thumbnails, and progress events.

The MVP release target is Windows 10/11 x64. Core crates remain portable so macOS and Linux can be qualified
later without changing project or render formats.

## Consequences

The existing static prototype remains a visual reference only. Business rules may not be duplicated in React.
Every IPC request is versioned, bounded, typed on both sides, and rejected when versions disagree.

