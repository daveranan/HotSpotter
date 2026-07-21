# Hot Trimmer

**Turn material photos into authored trim sheets and exportable PBR maps.**

Hot Trimmer is a focused desktop application for 3D artists who want to turn reference photos and material captures into structured trim-sheet assets. It combines source registration, patch authoring, layout tools, GPU-backed material processing, preview, and export in one local workflow.

Instead of trying to replace Blender or become a general-purpose material library, Hot Trimmer concentrates on one path:

```text
Sources -> Workbench -> Hotspot Sheet -> Processing -> Export
```

## Why Hot Trimmer?

Building a trim sheet from photographs normally means moving between image editors, layout tools, material utilities, and a DCC application. That makes iteration slow and makes it difficult to preserve the relationship between original captures, corrected patches, layout decisions, and generated maps.

Hot Trimmer keeps that work in a single project. Source images remain traceable, edits are non-destructive, layout decisions are explicit, and generated outputs come from a repeatable pipeline.

## Current Features

- Create, open, save, recover, and version local `.hottrimmer` projects.
- Import PNG, JPEG, and TIFF material captures.
- Register related PBR inputs such as base color, normal, roughness, height, metallic, and ambient occlusion.
- Inspect large sources through a mipmapped pan-and-zoom viewport.
- Capture rectangular, four-point, and assisted-polygon patches.
- Apply perspective correction and edit patch geometry non-destructively.
- Author trim-sheet regions with direct, repeating, and radial sampling behavior.
- Preview the material through a GPU-backed `wgpu` rendering pipeline.
- Inspect base color, normal, height, roughness, metallic, ambient-occlusion, and edge-mask outputs.
- Add and tune material-processing effects through the Processing workspace.
- Export enabled material maps and package metadata.
- Preserve deterministic project state, provenance, autosave, recovery, and revision-aware output behavior.

## Demo Workflow

1. Start Hot Trimmer and create a project.
2. Import a material photograph as the base-color source.
3. Add related PBR maps when available.
4. Capture and correct useful source patches in the Workbench.
5. Build the authored layout in Hotspot Sheet.
6. Inspect and refine the generated maps in Processing.
7. Use **Export All Maps** to write the enabled outputs.

No special sample data is required. A well-lit, front-facing photograph of brick, wood, stone, architectural trim, or another structured surface is enough to exercise the main workflow.

## Running the Project

### Supported platform

The current qualified target is **Windows 10/11 x64**. The Rust and Tauri architecture is largely portable, but macOS and Linux builds have not yet completed product qualification.

### Prerequisites

- Node.js 24
- Rust (the repository's `rust-toolchain.toml` selects the expected toolchain)
- Microsoft C++ Build Tools with the Desktop C++ workload
- WebView2 Runtime, included with current Windows installations

### Development

```powershell
npm.cmd install
npm.cmd run check
npm.cmd run dev
```

`npm run dev` starts the native Tauri application with the Vite development server.

### Native build

```powershell
npm.cmd run build:native
```

The executable is written to:

```text
target/release/hot-trimmer-desktop.exe
```

The current command creates a native executable without an installer bundle or code signature.

## Technology

- **Tauri 2** for the native desktop shell and platform services
- **React 19 + TypeScript** for the interface and editing workspaces
- **Rust** for project state, geometry, image processing, rendering, and export
- **wgpu + WGSL** for GPU-backed material-map generation and preview
- **SQLite via rusqlite** for durable, versioned project storage
- **Vite** for the frontend development and build pipeline
- **Blender Python API** for the companion integration under development

## Project Structure

```text
apps/desktop/        Tauri desktop application and React interface
crates/              Rust domain, storage, geometry, rendering, and export crates
packages/            Shared TypeScript UI, editor, and IPC contracts
integrations/blender Blender companion add-on
assets/              Built-in trim-sheet templates
fixtures/            Cross-language, project, and rendering test fixtures
docs/                Product decisions, technical specifications, and phase reports
```

The frontend communicates with the Rust application through versioned, typed Tauri commands. Project persistence and source ownership live in Rust; the React layer presents projections of that authoritative state. Rendering is tile-aware and revision-aware so that large outputs can be generated within explicit memory limits without publishing stale work.

## Built with Codex and GPT-5.6

Hot Trimmer was developed during OpenAI Build Week with Codex and GPT-5.6 as active engineering collaborators.

Codex helped turn product goals into bounded implementation plans, inspect the existing code before changes, implement features across the React/Rust boundary, run focused verification, and diagnose failures. It was especially useful during the migration from CPU-bound rendering to a GPU-backed `wgpu` pipeline, where changes had to remain consistent across domain contracts, WGSL shaders, application commands, fixtures, and tests.

GPT-5.6 was used through Codex for architectural reasoning, implementation, debugging, code review, and verification. Key collaboration areas included:

- Designing versioned domain and IPC contracts shared by Rust and TypeScript.
- Building durable project persistence, recovery, and revision semantics.
- Decomposing the rendering migration into testable stages.
- Implementing GPU atlas generation and material-map processing.
- Tracing behavior through fixtures and focused regression tests.
- Reviewing platform assumptions and release readiness.

The human-directed decisions remained the product scope, creative workflow, interaction model, visual priorities, acceptance criteria, and final tradeoffs. GPT-5.6 is a development-time collaborator; Hot Trimmer does not require an OpenAI API key and does not send project images or files to an AI service at runtime.

The repository's dated commit history and implementation reports document the work completed during the hackathon period.

## Verification

Run the full project gate with:

```powershell
npm.cmd run check
```

This covers repository contracts, TypeScript checks and tests, Rust formatting and linting, and the Rust workspace test suite. GPU and Blender behavioral fixtures may require compatible local hardware or a Blender installation for their dedicated checks.

## Known Limitations

- Windows is currently the only qualified desktop target.
- **Send to Blender** is not yet enabled in the desktop workflow.
- The native build is currently unsigned and is not packaged as an installer.
- GPU behavior and performance vary by adapter and driver.
- Some processing and layout workflows are still being refined as the project moves from hackathon prototype toward a distributable release.

## Roadmap

- Complete and qualify the Blender handoff workflow.
- Package signed Windows releases and add a streamlined first-run experience.
- Add macOS build and runtime qualification.
- Expand templates, sample projects, and visual documentation.
- Continue improving GPU scalability and material-processing controls.

## License

Hot Trimmer is open source under the [MIT License](LICENSE).
